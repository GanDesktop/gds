use super::super::*;
use russh::{
    client,
    keys::{self, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate},
};
use russh_sftp::{
    client::SftpSession,
    protocol::{FileAttributes, OpenFlags},
};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};
use tokio::sync::{Mutex, mpsc, oneshot};

pub(super) struct Client {
    ssh: Mutex<client::Handle<Verifier>>,
    files: SftpSession,
}

struct Verifier {
    host: String,
    port: u16,
    known_hosts: PathBuf,
    challenges: mpsc::Sender<HostKeyChallenge>,
}

impl client::Handler for Verifier {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        offered: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = offered.public_key();
        let known =
            keys::known_hosts::known_host_keys_path(&self.host, self.port, &self.known_hosts)?;
        if known.iter().any(|(_, old)| old == &key) {
            return Ok(true);
        }
        let changed = !known.is_empty();
        let (answer, reply) = oneshot::channel();
        self.challenges
            .send(HostKeyChallenge {
                host: self.host.clone(),
                port: self.port,
                fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
                changed,
                answer,
            })
            .await
            .map_err(|_| Error::Cancelled)?;
        if !reply.await.unwrap_or(false) {
            return Ok(false);
        }
        let store = crate::persistence::File::new(self.known_hosts.clone());
        let previous = store.read()?;
        let host = if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        };
        // Only app-owned records for this exact authority are replaced.
        let mut text = String::from_utf8(previous.clone().unwrap_or_default())?
            .lines()
            .filter(|line| line.split_whitespace().next() != Some(host.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        text.push_str(&format!("\n{host} {}\n", key.to_openssh()?));
        store.write(previous.as_deref(), text.as_bytes())?;
        Ok(true)
    }
}

impl Client {
    pub(super) async fn connect(
        address: &Address,
        options: ConnectOptions,
        challenges: mpsc::Sender<HostKeyChallenge>,
    ) -> Result<Self, Error> {
        let verifier = Verifier {
            host: address.host().into(),
            port: address.port(),
            known_hosts: options.known_hosts,
            challenges,
        };
        let config = Arc::new(client::Config {
            nodelay: true,
            ..Default::default()
        });
        let mut ssh = client::connect(config, (address.host(), address.port()), verifier)
            .await
            .map_err(|e| Error::protocol("SSH connection", e))?;
        let user = address.username();
        let credentials = options.credentials;
        let mut authenticated = false;
        if let Some(path) = credentials.private_key {
            let key = keys::load_secret_key(path, credentials.key_passphrase.as_deref())
                .map_err(|e| Error::protocol("SSH key", e))?;
            let hash = ssh
                .best_supported_rsa_hash()
                .await
                .map_err(|e| Error::protocol("SSH algorithms", e))?
                .flatten();
            authenticated = ssh
                .authenticate_publickey(&user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await
                .map_err(|e| Error::protocol("SSH key authentication", e))?
                .success();
        }
        #[cfg(unix)]
        if !authenticated
            && credentials.password.is_empty()
            && std::env::var_os("SSH_AUTH_SOCK").is_some()
        {
            let mut agent = keys::agent::client::AgentClient::connect_env()
                .await
                .map_err(|e| Error::protocol("SSH agent", e))?;
            for identity in agent
                .request_identities()
                .await
                .map_err(|e| Error::protocol("SSH agent identities", e))?
            {
                let hash = ssh
                    .best_supported_rsa_hash()
                    .await
                    .map_err(|e| Error::protocol("SSH algorithms", e))?
                    .flatten();
                if ssh
                    .authenticate_publickey_with(
                        &user,
                        identity.public_key().into_owned(),
                        hash,
                        &mut agent,
                    )
                    .await
                    .map_err(|e| Error::protocol("SSH agent authentication", e))?
                    .success()
                {
                    authenticated = true;
                    break;
                }
            }
        }
        if !authenticated {
            authenticated = ssh
                .authenticate_password(&user, credentials.password)
                .await
                .map_err(|e| Error::protocol("SSH password authentication", e))?
                .success();
        }
        if !authenticated {
            return Err(Error::Authentication);
        }
        let channel = ssh
            .channel_open_session()
            .await
            .map_err(|e| Error::protocol("SSH channel", e))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| Error::protocol("SFTP subsystem", e))?;
        let files = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| Error::protocol("SFTP initialization", e))?;
        Ok(Self {
            ssh: Mutex::new(ssh),
            files,
        })
    }

    fn entry(address: Address, metadata: FileAttributes) -> Entry {
        let kind = if metadata.is_dir() {
            EntryKind::Directory
        } else if metadata.is_symlink() {
            EntryKind::SymbolicLink
        } else if metadata.is_regular() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        Entry {
            name: address.name(),
            address,
            kind,
            size: if kind == EntryKind::File {
                metadata.size
            } else {
                None
            },
            modified: metadata
                .mtime
                .map(|v| UNIX_EPOCH + Duration::from_secs(v as u64)),
        }
    }
    pub(super) async fn list(&self, address: &Address) -> Result<Vec<Entry>, Error> {
        let entries = self
            .files
            .read_dir(address.path())
            .await
            .map_err(|e| Error::protocol("SFTP list", e))?;
        entries
            .map(|entry| {
                let name = entry.file_name();
                // russh-sftp decodes invalid UTF-8 lossily; never address a different file by that name.
                if name.contains(char::REPLACEMENT_CHARACTER) {
                    return Err(Error::Unsupported(
                        "SFTP names that cannot be decoded unambiguously as UTF-8",
                    ));
                }
                Ok(Self::entry(address.child(&name)?, entry.metadata()))
            })
            .collect()
    }
    pub(super) async fn stat(&self, address: &Address) -> Result<Entry, Error> {
        Ok(Self::entry(
            address.clone(),
            self.files
                .symlink_metadata(address.path())
                .await
                .map_err(|e| Error::protocol("SFTP metadata", e))?,
        ))
    }
    pub(super) async fn mkdir(&self, address: &Address) -> Result<(), Error> {
        self.files
            .create_dir(address.path())
            .await
            .map_err(|e| Error::protocol("SFTP mkdir", e))
    }
    pub(super) async fn rename(&self, from: &Address, to: &Address) -> Result<(), Error> {
        // SFTP v3 RENAME refuses an occupied destination; do not use posix-rename's overwrite extension.
        self.files
            .rename(from.path(), to.path())
            .await
            .map_err(|e| Error::protocol("SFTP rename", e))
    }
    pub(super) async fn remove(&self, address: &Address, kind: EntryKind) -> Result<(), Error> {
        let result = if kind == EntryKind::Directory {
            self.files.remove_dir(address.path()).await
        } else {
            self.files.remove_file(address.path()).await
        };
        result.map_err(|e| Error::protocol("SFTP remove", e))
    }
    pub(super) async fn open(
        &self,
        address: &Address,
        create: bool,
    ) -> Result<super::Stream, Error> {
        let flags = if create {
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE
        } else {
            OpenFlags::READ
        };
        let file = self
            .files
            .open_with_flags(address.path(), flags)
            .await
            .map_err(|e| Error::protocol("SFTP open", e))?;
        Ok(super::Stream::sftp(file))
    }
    pub(super) async fn close(&self) -> Result<(), Error> {
        self.files
            .close()
            .await
            .map_err(|e| Error::protocol("SFTP disconnect", e))?;
        self.ssh
            .lock()
            .await
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await
            .map_err(|e| Error::protocol("SSH disconnect", e))
    }
}
