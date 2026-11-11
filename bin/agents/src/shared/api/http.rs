//! HTTP transport for GPUI-managed assets, using the application task supervisor.

use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use futures::future::BoxFuture;
use gds::service::task_manager::Scope;
use gpui_kit::http_client::{
    AsyncBody, HttpClient, Request, RequestTimeout, Response, Url, http::HeaderValue,
};
use reqwest_client::ReqwestClient;

const USER_AGENT: &str = concat!("gds/agents/", env!("CARGO_PKG_VERSION"));
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

struct AssetClient {
    inner: ReqwestClient,
    tasks: Scope,
}

impl HttpClient for AssetClient {
    fn user_agent(&self) -> Option<&HeaderValue> {
        self.inner.user_agent()
    }

    fn proxy(&self) -> Option<&Url> {
        self.inner.proxy()
    }

    /// # Errors
    /// Propagates transport, TLS, and request-deadline failures.
    fn send(
        &self,
        mut request: Request<AsyncBody>,
    ) -> BoxFuture<'static, Result<Response<AsyncBody>>> {
        if request.extensions().get::<RequestTimeout>().is_none() {
            request
                .extensions_mut()
                .insert(RequestTimeout(REQUEST_TIMEOUT));
        }
        let work = self.inner.send(request);
        let task = self.tasks.spawn_cancellable(work).cancel_on_drop();
        Box::pin(async move { task.await? })
    }
}

/// Creates the transport before any GPUI window can request a remote asset.
///
/// # Errors
/// Returns an error if HTTP or TLS client configuration cannot be initialized.
pub(crate) async fn client(tasks: &Scope) -> Result<Arc<dyn HttpClient>> {
    // The transport captures Tokio context while constructed by the supervisor.
    let inner = tasks
        .spawn(async {
            ReqwestClient::proxy_and_user_agent(None, USER_AGENT)
                .context("cannot initialize the GUI asset HTTP client")
        })
        .await??;
    Ok(Arc::new(AssetClient {
        inner,
        tasks: tasks.clone(),
    }))
}
