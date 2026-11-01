use super::Error;
use gds::service::remote::{Address, EntryKind, Session};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
    pub(super) async fn mkdir(&self) -> Result<(), Error> {
        match self {
            Self::Local(path) => Ok(tokio::fs::create_dir(path).await?),
            Self::Remote { address, session } => Ok(session.mkdir(address).await?),
        }
    }
    pub(super) async fn remove(&self, kind: EntryKind) -> Result<(), Error> {
        match self {
            Self::Local(path) if kind == EntryKind::Directory => {
                Ok(tokio::fs::remove_dir(path).await?)
            }
            Self::Local(path) => Ok(tokio::fs::remove_file(path).await?),
            Self::Remote { address, session } => Ok(session.remove(address, kind).await?),
        }
    }
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
    pub(super) async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        match self {
            Self::Local(file) => Ok(file.read(buffer).await?),
            Self::Remote(file) => Ok(file.read(buffer).await?),
        }
    }
    pub(super) async fn write_all(&mut self, buffer: &[u8]) -> Result<(), Error> {
        match self {
            Self::Local(file) => Ok(file.write_all(buffer).await?),
            Self::Remote(file) => Ok(file.write_all(buffer).await?),
        }
    }
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
