use super::Error;
use gds::service::remote::{Address, EntryKind, Session};
use std::{future::Future, path::PathBuf, pin::Pin};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) enum Resource {
    Local(PathBuf),
    Remote { address: Address, session: Session },
}

impl Resource {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Local(path) => path.display().to_string(),
            Self::Remote { address, .. } => address.to_string(),
        }
    }

    /// # Errors
    /// Rejects local paths without a valid UTF-8 leaf name.
    pub(crate) fn name(&self) -> Result<String, Error> {
        match self {
            Self::Local(path) => path
                .file_name()
                .and_then(|s| s.to_str())
                .map(str::to_owned)
                .ok_or(Error::Unsupported(
                    "remote transfers require UTF-8 leaf names",
                )),
            Self::Remote { address, .. } => Ok(address.name()),
        }
    }

    /// # Errors
    /// Rejects invalid leaf names for remote protocols.
    pub(crate) fn child(&self, name: &str) -> Result<Self, Error> {
        Address::validate_name(name, gds::service::remote::Protocol::Sftp)?;
        Ok(match self {
            Self::Local(path) => Self::Local(path.join(name)),
            Self::Remote { address, session } => Self::Remote {
                address: address.child(name)?,
                session: session.clone(),
            },
        })
    }

    /// # Errors
    /// Reports metadata inspection failures on local or remote storage.
    pub(super) async fn kind(&self) -> Result<EntryKind, Error> {
        match self {
            Self::Local(path) => {
                let m = tokio::fs::symlink_metadata(path).await?;
                Ok(if m.is_symlink() {
                    EntryKind::SymbolicLink
                } else if m.is_dir() {
                    EntryKind::Directory
                } else if m.is_file() {
                    EntryKind::File
                } else {
                    EntryKind::Other
                })
            }
            Self::Remote { address, session } => Ok(session.stat(address).await?.kind),
        }
    }

    /// # Errors
    /// Reports directory listing failures on local or remote storage.
    pub(super) async fn children(&self) -> Result<Vec<Self>, Error> {
        match self {
            Self::Local(path) => {
                let mut dir = tokio::fs::read_dir(path).await?;
                let mut children = Vec::new();
                while let Some(entry) = dir.next_entry().await? {
                    children.push(Self::Local(entry.path()));
                }
                Ok(children)
            }
            Self::Remote { address, session } => Ok(session
                .list(address)
                .await?
                .entries
                .into_iter()
                .map(|entry| Self::Remote {
                    address: entry.address,
                    session: session.clone(),
                })
                .collect()),
        }
    }

    /// # Errors
    /// Reports directory creation failures on local or remote storage.
    pub(super) async fn mkdir(&self) -> Result<(), Error> {
        match self {
            Self::Local(path) => Ok(tokio::fs::create_dir(path).await?),
            Self::Remote { address, session } => Ok(session.mkdir(address).await?),
        }
    }

    /// # Errors
    /// Reports removal failures on local or remote storage.
    pub(super) async fn remove(&self, kind: EntryKind) -> Result<(), Error> {
        match self {
            Self::Local(path) if kind == EntryKind::Directory => {
                Ok(tokio::fs::remove_dir(path).await?)
            }
            Self::Local(path) => Ok(tokio::fs::remove_file(path).await?),
            Self::Remote { address, session } => Ok(session.remove(address, kind).await?),
        }
    }

    /// Recursively remove a resource tree without following symbolic links.
    ///
    /// # Errors
    /// Reports cancellation or local/remote removal failures.
    pub(super) fn remove_tree<'a>(
        &'a self,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let kind = self.kind().await?;
            if kind == EntryKind::Directory {
                for child in self.children().await? {
                    child.remove_tree(cancel).await?;
                }
            }
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            self.remove(kind).await
        })
    }

    /// # Errors
    /// Reports stream open or exclusive creation failures.
    pub(super) async fn stream(&self, create: bool) -> Result<Stream, Error> {
        Ok(match self {
            Self::Local(path) => Stream::Local(if create {
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .await?
            } else {
                tokio::fs::File::open(path).await?
            }),
            Self::Remote { address, session } => Stream::Remote(if create {
                session.create_new(address).await?
            } else {
                session.open(address).await?
            }),
        })
    }
}

pub(super) enum Stream {
    Local(tokio::fs::File),
    Remote(gds::service::remote::Stream),
}

impl Stream {
    /// # Errors
    /// Propagates stream read errors.
    pub(super) async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        match self {
            Self::Local(file) => Ok(file.read(buffer).await?),
            Self::Remote(file) => Ok(file.read(buffer).await?),
        }
    }

    /// # Errors
    /// Propagates stream write errors.
    pub(super) async fn write_all(&mut self, buffer: &[u8]) -> Result<(), Error> {
        match self {
            Self::Local(file) => Ok(file.write_all(buffer).await?),
            Self::Remote(file) => Ok(file.write_all(buffer).await?),
        }
    }

    /// # Errors
    /// Propagates flush, sync or remote close errors.
    pub(super) async fn close(self) -> Result<(), Error> {
        match self {
            Self::Local(mut file) => {
                file.flush().await?;
                file.sync_all().await?;
                Ok(())
            }
            Self::Remote(file) => Ok(file.close().await?),
        }
    }
}
