mod address;
pub use address::{Address, Protocol};

use std::{path::PathBuf, time::SystemTime};
use tokio::sync::oneshot;

/// Connection secrets intentionally have no Debug/Serialize implementation;
/// optional persistence goes through [`keyring`](crate::service::remote::keyring).
#[derive(Default, Clone)]
pub struct Credentials {
    pub password: String,
    pub private_key: Option<PathBuf>,
    pub key_passphrase: Option<String>,
}

impl Credentials {
    /// Returns whether these credentials carry a secret or private key path
    /// that can be stored in the system keyring.
    #[must_use]
    pub fn has_secret(&self) -> bool {
        !self.password.is_empty() || self.private_key.is_some()
    }
}

pub struct ConnectOptions {
    pub credentials: Credentials,
    pub known_hosts: PathBuf,
}

/// An SSH host key must be approved before authentication sends any credentials.
pub struct HostKeyChallenge {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub changed: bool,
    pub answer: oneshot::Sender<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    SymbolicLink,
    Other,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub address: Address,
    pub name: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
pub struct Listing {
    pub address: Address,
    pub entries: Vec<Entry>,
}
