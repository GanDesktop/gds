use std::{fs::File, path::Path};

use ashpd::desktop::{ResponseError, open_uri::OpenFileRequest};

use super::{OpenMode, OpenOutcome};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("无法连接桌面 Portal 会话总线：{0}")]
    Connection(#[source] zbus::Error),
    #[error("无法读取文件 {path}：{source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("无法通过桌面 Portal 打开文件 {path}：{source}")]
    Request {
        path: std::path::PathBuf,
        #[source]
        source: ashpd::Error,
    },
    #[error("默认应用打开失败：{default}；选择其他应用也未能完成：{chooser}")]
    DefaultAndChooser {
        default: Box<Error>,
        #[source]
        chooser: Box<Error>,
    },
}

pub(super) struct Portal {
    connection: zbus::Connection,
}

impl Portal {
    /// Connect on Tokio so zbus uses its Tokio backend. GPUI enables ashpd's
    /// async-io feature and may initialize its process-wide connection first.
    ///
    /// # Errors
    /// Reports an unavailable session bus.
    pub(super) async fn connect() -> Result<Self, Error> {
        let connection = zbus::Connection::session()
            .await
            .map_err(Error::Connection)?;
        Ok(Self { connection })
    }

    /// # Errors
    /// Reports unavailable files, portal failures and application launch failures.
    pub(super) async fn open_file(
        &self,
        path: &Path,
        mode: OpenMode,
    ) -> Result<OpenOutcome, Error> {
        match mode {
            OpenMode::ChooseApplication => self.request(path, true).await,
            OpenMode::DefaultApplication => {
                Self::default_or_choose(|ask| self.request(path, ask)).await
            }
        }
    }

    /// # Errors
    /// Reports the default-handler error together with a failed application chooser.
    async fn default_or_choose<F, Fut>(mut send: F) -> Result<OpenOutcome, Error>
    where
        F: FnMut(bool) -> Fut,
        Fut: std::future::Future<Output = Result<OpenOutcome, Error>>,
    {
        match send(false).await {
            Ok(outcome) => Ok(outcome),
            Err(default_error) => {
                tracing::warn!(%default_error, "default document open failed; asking for another application");
                send(true)
                    .await
                    .map_err(|chooser| Error::DefaultAndChooser {
                        default: Box::new(default_error),
                        chooser: Box::new(chooser),
                    })
            }
        }
    }

    /// # Errors
    /// Reports unreadable files, portal availability and application launch failures.
    async fn request(&self, path: &Path, ask: bool) -> Result<OpenOutcome, Error> {
        let file = File::open(path).map_err(|source| Error::Read {
            path: path.to_owned(),
            source,
        })?;
        // OpenFile passes an fd, preserving non-UTF-8 names and asking the desktop
        // to open a document instead of executing the selected file.
        // https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.OpenURI.html
        let response = OpenFileRequest::default()
            .connection(Some(self.connection.clone()))
            .ask(ask)
            .send_file(&file)
            .await
            .and_then(|request| request.response());
        match response {
            Ok(()) => Ok(OpenOutcome::Opened),
            Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(OpenOutcome::Cancelled),
            Err(source) => Err(Error::Request {
                path: path.to_owned(),
                source,
            }),
        }
    }
}

#[cfg(test)]
mod tests;
