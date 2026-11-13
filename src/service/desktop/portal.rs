use std::{
    fs::File,
    io::{Seek, SeekFrom},
    path::Path,
};

use ashpd::desktop::{ResponseError, open_uri::OpenFileRequest};

use super::{OpenMode, OpenOutcome};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot connect to the desktop portal session bus: {0}")]
    Connection(#[source] zbus::Error),
    #[error("cannot read file {path}: {source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot open file {path} through the desktop portal: {source}")]
    Request {
        path: std::path::PathBuf,
        #[source]
        source: ashpd::Error,
    },
    #[error(
        "opening with the default application failed: {default}; choosing another application also failed: {chooser}"
    )]
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
    /// Reports unreadable files, portal failures and application launch failures.
    pub(super) async fn open_file(
        &self,
        path: &Path,
        file: &File,
        mode: OpenMode,
    ) -> Result<OpenOutcome, Error> {
        match mode {
            OpenMode::ChooseApplication => self.request(path, file, true).await,
            OpenMode::DefaultApplication => {
                Self::default_or_choose(path, file, |ask| self.request(path, file, ask)).await
            }
        }
    }

    fn rewind(path: &Path, mut file: &File) -> Result<(), Error> {
        Seek::seek(&mut file, SeekFrom::Start(0))
            .map(|_| ())
            .map_err(|source| Error::Read {
                path: path.to_owned(),
                source,
            })
    }

    /// # Errors
    /// Reports the default-handler error together with a failed application chooser.
    async fn default_or_choose<F, Fut>(
        path: &Path,
        file: &File,
        mut send: F,
    ) -> Result<OpenOutcome, Error>
    where
        F: FnMut(bool) -> Fut,
        Fut: std::future::Future<Output = Result<OpenOutcome, Error>>,
    {
        match send(false).await {
            Ok(outcome) => Ok(outcome),
            Err(default_error) => {
                tracing::warn!(%default_error, "default document open failed; asking for another application");
                if let Err(chooser_error) = Self::rewind(path, file) {
                    return Err(Error::DefaultAndChooser {
                        default: Box::new(default_error),
                        chooser: Box::new(chooser_error),
                    });
                }
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
    async fn request(&self, path: &Path, file: &File, ask: bool) -> Result<OpenOutcome, Error> {
        Self::rewind(path, file)?;
        // OpenFile passes an fd, preserving non-UTF-8 names and asking the desktop
        // to open a document instead of executing the selected file.
        // https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.OpenURI.html
        let response = OpenFileRequest::default()
            .connection(Some(self.connection.clone()))
            .ask(ask)
            .send_file(file)
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
