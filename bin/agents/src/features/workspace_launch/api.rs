mod directory;
use crate::shared::i18n::tr;
use bas::service::{Client, pb, tls::ConnectionOptions};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio_util::task::AbortOnDropHandle;

#[derive(Clone)]
pub(super) struct Service {
    pub target: ConnectionOptions,
    pub local: bool,
    pub runtime: Handle,
}

impl Service {
    pub fn request<T: Send + 'static, F>(
        &self,
        operation: F,
    ) -> impl Future<Output = Result<T, String>> + use<T, F>
    where
        F: FnOnce(Client) -> std::pin::Pin<Box<dyn Future<Output = Result<T, String>> + Send>>
            + Send
            + 'static,
    {
        let target = self.target.clone();
        let task = AbortOnDropHandle::new(self.runtime.spawn(async move {
            tokio::time::timeout(Duration::from_secs(60), async {
                let client = Client::connect_options(&target)
                    .await
                    .map_err(|error| error.to_string())?;
                operation(client).await
            })
            .await
            .map_err(|error| error.to_string())?
        }));
        async move { task.await.map_err(|error| error.to_string())? }
    }

    pub fn providers(
        &self,
    ) -> impl Future<Output = Result<Vec<pb::WorkspaceProvider>, String>> + use<> {
        self.request(|mut client| {
            Box::pin(async move {
                client
                    .list_workspace_providers(pb::ListWorkspaceProvidersRequest {})
                    .await
                    .map(|response| response.into_inner().providers)
                    .map_err(|error| {
                        if error.code() == tonic::Code::Unimplemented {
                            tr!("workspace-provider-upgrade").into()
                        } else {
                            error.to_string()
                        }
                    })
            })
        })
    }

    pub fn browse(
        &self,
        path: Option<String>,
        cursor: Option<String>,
    ) -> impl Future<Output = Result<gds::ui::picker::directory::Page, String>> + use<> {
        self.request(move |mut client| {
            Box::pin(async move {
                client
                    .browse_server_directories(pb::BrowseServerDirectoriesRequest { path, cursor })
                    .await
                    .map(|response| directory::page(response.into_inner()))
                    .map_err(|error| error.to_string())
            })
        })
    }

    pub fn choices(
        &self,
        request: pb::QueryWorkspaceChoicesRequest,
    ) -> impl Future<Output = Result<pb::QueryWorkspaceChoicesResponse, String>> + use<> {
        self.request(move |mut client| {
            Box::pin(async move {
                client
                    .query_workspace_choices(request)
                    .await
                    .map(|response| response.into_inner())
                    .map_err(|error| error.to_string())
            })
        })
    }

    pub fn open(
        &self,
        request: pb::OpenWorkspaceRequest,
    ) -> impl Future<Output = Result<bas::model::Workspace, String>> + use<> {
        self.request(move |mut client| {
            Box::pin(async move {
                client
                    .open_workspace(request)
                    .await
                    .map_err(|error| error.to_string())?
                    .into_inner()
                    .try_into()
                    .map_err(|error: bas::service::Error| error.to_string())
            })
        })
    }
}
