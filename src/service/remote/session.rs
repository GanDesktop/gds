mod sftp;
mod smb;
mod stream;
pub use stream::Stream;

use super::*;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Session {
    key: String,
    backend: Arc<Backend>,
}

enum Backend {
    Sftp(sftp::Client),
    Smb(smb::Client),
}

impl Session {
    /// # Errors
    /// Reports connection, authentication, host verification and protocol failures.
    pub async fn connect(
        address: &Address,
        options: ConnectOptions,
        challenges: mpsc::Sender<HostKeyChallenge>,
    ) -> Result<Self, Error> {
        let backend = match address.protocol() {
            Protocol::Sftp => {
                Backend::Sftp(sftp::Client::connect(address, options, challenges).await?)
            }
            Protocol::Smb => {
                Backend::Smb(smb::Client::connect(address, options.credentials).await?)
            }
        };
        Ok(Self {
            key: address.connection_key(),
            backend: Arc::new(backend),
        })
    }

    fn check(&self, address: &Address) -> Result<(), Error> {
        if self.key == address.connection_key() {
            Ok(())
        } else {
            Err(Error::WrongConnection)
        }
    }

    /// # Errors
    /// Reports a different connection identity, invalid entries, or enumeration failure.
    pub async fn list(&self, address: &Address) -> Result<Listing, Error> {
        self.check(address)?;
        let mut entries = match self.backend.as_ref() {
            Backend::Sftp(client) => client.list(address).await?,
            Backend::Smb(client) => client.list(address).await?,
        };
        entries.sort_by(|a, b| {
            (b.kind == EntryKind::Directory)
                .cmp(&(a.kind == EntryKind::Directory))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(Listing {
            address: address.clone(),
            entries,
        })
    }

    /// # Errors
    /// Reports missing entries, a different connection, or unavailable metadata.
    pub async fn stat(&self, address: &Address) -> Result<Entry, Error> {
        self.check(address)?;
        match self.backend.as_ref() {
            Backend::Sftp(client) => client.stat(address).await,
            Backend::Smb(client) => client.stat(address).await,
        }
    }

    /// # Errors
    /// Reports occupied names, permissions or protocol failure. Existing directories are not reused.
    pub async fn mkdir(&self, address: &Address) -> Result<(), Error> {
        self.check(address)?;
        match self.backend.as_ref() {
            Backend::Sftp(client) => client.mkdir(address).await,
            Backend::Smb(client) => client.mkdir(address).await,
        }
    }

    /// # Errors
    /// Reports occupied destinations, different connections or protocol failure.
    pub async fn rename(&self, from: &Address, to: &Address) -> Result<(), Error> {
        self.check(from)?;
        self.check(to)?;
        match self.backend.as_ref() {
            Backend::Sftp(client) => client.rename(from, to).await,
            Backend::Smb(client) => client.rename(from, to).await,
        }
    }

    /// # Errors
    /// Reports non-empty directories, permissions or protocol failure. Does not follow symlinks.
    pub async fn remove(&self, address: &Address, kind: EntryKind) -> Result<(), Error> {
        self.check(address)?;
        match self.backend.as_ref() {
            Backend::Sftp(client) => client.remove(address, kind).await,
            Backend::Smb(client) => client.remove(address, kind).await,
        }
    }

    /// # Errors
    /// Reports unavailable files or protocol failure.
    pub async fn open(&self, address: &Address) -> Result<Stream, Error> {
        self.check(address)?;
        match self.backend.as_ref() {
            Backend::Sftp(client) => client.open(address, false).await,
            Backend::Smb(client) => client.open(address, false).await,
        }
    }

    /// # Errors
    /// Refuses an occupied name at creation time and reports protocol failures.
    pub async fn create_new(&self, address: &Address) -> Result<Stream, Error> {
        self.check(address)?;
        match self.backend.as_ref() {
            Backend::Sftp(client) => client.open(address, true).await,
            Backend::Smb(client) => client.open(address, true).await,
        }
    }

    /// # Errors
    /// Reports a failed disconnect. Call after this session's active tasks have ended.
    pub async fn close(&self) -> Result<(), Error> {
        match self.backend.as_ref() {
            Backend::Sftp(client) => client.close().await,
            Backend::Smb(client) => client.close().await,
        }
    }
}
