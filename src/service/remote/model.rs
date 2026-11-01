mod address;
pub use address::{Address, Protocol};

use std::{path::PathBuf, time::SystemTime};
use tokio::sync::oneshot;

/// Connection secrets are memory-only and intentionally have no Debug/Serialize implementation.
#[derive(Default, Clone)]
pub struct Credentials {
    pub password: String,
    pub private_key: Option<PathBuf>,
    pub key_passphrase: Option<String>,
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
