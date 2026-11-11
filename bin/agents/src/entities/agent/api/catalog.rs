//! Daemon-owned discovery and installation; no settings-page state or presentation.

use crate::shared::i18n::tr;
use bas::service::tls::ConnectionOptions as Target;
use std::time::Duration;

use bas::{
    config::AgentSpec,
    management::{Catalog, Phase},
    service::{Client, pb},
};
use futures::{StreamExt, channel::mpsc};
use gds::service::task_manager::Scope;
use gds::service::task_manager::Task as IoTask;
use gpui_kit::{App, Global};

const CATALOG_TIMEOUT: Duration = Duration::from_secs(45);

mod policy;

#[derive(Clone)]
pub(crate) struct Service {
    server: Target,
    tasks: Scope,
}
impl Global for Service {}

pub(in crate::entities::agent) fn init(server: Target, tasks: Scope, cx: &mut App) {
    cx.set_global(Service { server, tasks });
}

pub(crate) struct Progress {
    pub phase: Phase,
    pub complete: bool,
    pub spec: Option<AgentSpec>,
}

fn decode(operation: pb::AgentOperation) -> Progress {
    let spec = if operation.complete {
        serde_json::from_slice(&operation.spec_json).ok()
    } else {
        None
    };
    Progress {
        phase: operation.phase(),
        complete: operation.complete,
        spec,
    }
}

pub(crate) struct Updates {
    receiver: mpsc::UnboundedReceiver<Result<Progress, String>>,
    _task: IoTask<()>,
}

impl Updates {
    pub(crate) async fn next(&mut self) -> Option<Result<Progress, String>> {
        self.receiver.next().await
    }
}

impl Service {
    pub(crate) fn configuration(
        &self,
        save: Option<pb::SaveAgentConfigurationRequest>,
    ) -> impl Future<Output = Result<pb::AgentConfiguration, String>> + use<> {
        let server = self.server.clone();
        let task = IoTask::cancel_on_drop(self.tasks.spawn(async move {
            tokio::time::timeout(CATALOG_TIMEOUT, async {
                let mut client = Client::connect_options(&server)
                    .await
                    .map_err(|error| error.to_string())?;
                let response = match save {
                    Some(request) => client.save_agent_configuration(request).await,
                    None => {
                        client
                            .get_agent_configuration(pb::GetAgentConfigurationRequest {})
                            .await
                    }
                };
                response
                    .map(|response| response.into_inner())
                    .map_err(|error| {
                        if error.code() == tonic::Code::Unimplemented {
                            tr!("agent-config-upgrade").to_owned()
                        } else {
                            error.message().to_owned()
                        }
                    })
            })
            .await
            .unwrap_or_else(|_| Err(tr!("agent-config-timeout").into()))
        }));
        async move {
            task.await.unwrap_or_else(|error| {
                tracing::error!(%error, "Agent request task failed");
                Err(tr!("agent-config-ended").into())
            })
        }
    }

    pub(crate) fn new(server: Target, tasks: Scope) -> Self {
        Self { server, tasks }
    }

    pub(crate) fn address(&self) -> Target {
        self.server.clone()
    }

    pub(crate) fn tasks(&self) -> &Scope {
        &self.tasks
    }

    pub(crate) fn local(&self) -> bool {
        // A forwarded loopback port does not establish ownership of local files.
        self.server.is_local()
    }

    pub(crate) fn catalog(
        &self,
        refresh: bool,
    ) -> impl Future<Output = Result<Catalog, String>> + use<> {
        let server = self.server.clone();
        let task = IoTask::cancel_on_drop(self.tasks.spawn_cancellable(async move {
            tokio::time::timeout(CATALOG_TIMEOUT, async {
                let mut client = Client::connect_options(&server)
                    .await
                    .map_err(|error| error.to_string())?;
                let response = client
                    .list_agent_catalog(pb::ListAgentCatalogRequest { refresh })
                    .await
                    .map_err(|error| error.message().to_owned())?
                    .into_inner();
                serde_json::from_slice(&response.catalog_json).map_err(|error| error.to_string())
            })
            .await
            .unwrap_or_else(|_| Err(tr!("catalog-timeout").into()))
        }));
        async move {
            task.await.unwrap_or_else(|error| {
                tracing::error!(%error, "Agent request task failed");
                Err(tr!("catalog-ended").into())
            })
        }
    }

    pub(crate) fn install(&self, id: String) -> Updates {
        let server = self.server.clone();
        let (send, receiver) = mpsc::unbounded();
        let task = IoTask::cancel_on_drop(self.tasks.spawn_cancellable(async move {
            let result = async {
                let mut client = Client::connect_options(&server)
                    .await
                    .map_err(|error| error.to_string())?;
                let mut stream = client
                    .install_agent(pb::InstallAgentRequest { id })
                    .await
                    .map_err(|error| error.message().to_owned())?
                    .into_inner();
                while let Some(operation) = stream
                    .message()
                    .await
                    .map_err(|error| error.message().to_owned())?
                {
                    let complete = operation.complete;
                    if send.unbounded_send(Ok(decode(operation))).is_err() {
                        return Ok(());
                    }
                    if complete {
                        return Ok(());
                    }
                }
                Err(tr!("agent-install-incomplete").to_owned())
            }
            .await;
            if let Err(error) = result {
                let _ = send.unbounded_send(Err(error));
            }
        }));
        Updates {
            receiver,
            _task: task,
        }
    }

    pub(crate) fn remove(&self, id: String) -> impl Future<Output = Result<(), String>> + use<> {
        let server = self.server.clone();
        let task = IoTask::cancel_on_drop(self.tasks.spawn_cancellable(async move {
            async {
                let mut client = Client::connect_options(&server)
                    .await
                    .map_err(|error| error.to_string())?;
                client
                    .remove_agent(pb::RemoveAgentRequest { id })
                    .await
                    .map_err(|error| error.message().to_owned())?;
                Ok(())
            }
            .await
        }));
        async move {
            task.await.unwrap_or_else(|error| {
                tracing::error!(%error, "Agent request task failed");
                Err(tr!("agent-remove-ended").into())
            })
        }
    }

    pub(crate) fn logout(
        &self,
        spec: AgentSpec,
        settings_path: std::path::PathBuf,
    ) -> impl Future<Output = Result<(), String>> + use<> {
        let server = self.server.clone();
        let task = IoTask::cancel_on_drop(self.tasks.spawn_cancellable(async move {
            tokio::time::timeout(CATALOG_TIMEOUT, async {
                let mut client = super::check::verified_client(server, &settings_path).await?;
                let expected_spec_json =
                    serde_json::to_vec(&spec).map_err(|error| error.to_string())?;
                client
                    .logout_agent(pb::LogoutAgentRequest {
                        agent: spec.id.to_string(),
                        expected_spec_json,
                    })
                    .await
                    .map_err(|error| {
                        if error.code() == tonic::Code::Unimplemented {
                            tr!("agent-check-upgrade").into()
                        } else {
                            crate::shared::api::failure::Failure::from(error).presentation()
                        }
                    })?;
                Ok(())
            })
            .await
            .unwrap_or_else(|_| Err(tr!("agent-logout-unknown").into()))
        }));
        async move {
            task.await.unwrap_or_else(|error| {
                tracing::error!(%error, "Agent request task failed");
                Err(tr!("agent-logout-unknown").into())
            })
        }
    }
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
