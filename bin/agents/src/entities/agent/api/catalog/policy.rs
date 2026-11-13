use super::*;
use crate::shared::i18n::tr;

impl Service {
    pub(crate) fn acp_configuration(
        &self,
        save: Option<pb::SaveAcpConfigurationRequest>,
    ) -> impl Future<Output = Result<pb::AcpConfiguration, String>> + use<> {
        let server = self.server.clone();
        let task = IoTask::cancel_on_drop(self.tasks.spawn_named(
            "Read or save ACP configuration",
            async move {
                tokio::time::timeout(CATALOG_TIMEOUT, async {
                    let mut client = Client::connect_options(&server)
                        .await
                        .map_err(|e| e.to_string())?;
                    let response = match save {
                        Some(request) => client.save_acp_configuration(request).await,
                        None => {
                            client
                                .get_acp_configuration(pb::GetAcpConfigurationRequest {})
                                .await
                        }
                    };
                    response
                        .map(|response| response.into_inner())
                        .map_err(|error| {
                            if error.code() == tonic::Code::Unimplemented {
                                tr!("acp-policy-upgrade").into()
                            } else {
                                error.message().to_owned()
                            }
                        })
                })
                .await
                .unwrap_or_else(|_| Err(tr!("agent-config-timeout").into()))
            },
        ));
        async move {
            task.await.unwrap_or_else(|error| {
                tracing::error!(%error, "Agent request task failed");
                Err(tr!("agent-config-ended").into())
            })
        }
    }
}
