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
    borrow::Cow,
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};
use tokio::sync::{Mutex, mpsc, oneshot};

const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const SSH_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(60);
const SSH_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const SSH_KEEPALIVE_MAX: usize = 3;
const PROBE_PUBLIC_KEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";

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

fn known_hosts_authority(host: &str, port: u16) -> Cow<'_, str> {
    if port == 22 {
        Cow::Borrowed(host)
    } else {
        Cow::Owned(format!("[{host}]:{port}"))
    }
}

fn parse_known_hosts_line(line: &str) -> Option<(&str, &str, &str)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('@') {
        return None;
    }
    let hosts_end = trimmed.find(char::is_whitespace)?;
    let hosts = &trimmed[..hosts_end];
    let remainder = trimmed[hosts_end..].trim_start();
    let mut parts = remainder.split_whitespace();
    let line_algo = parts.next()?;
    let _key_blob = parts.next()?;
    Some((hosts, line_algo, remainder))
}

fn match_hashed_hosts(
    host: &str,
    port: u16,
    candidates: &[&str],
) -> anyhow::Result<HashSet<String>> {
    if candidates.is_empty() {
        return Ok(HashSet::new());
    }
    // Delegate HMAC-SHA1 verification of `|1|salt|hash` tokens to `russh`'s own
    // `known_host_keys_path` parser using a headerless temporary file where each
    // 1-based line number maps directly back to `candidates[line - 1]`.
    let mut probe = tempfile::NamedTempFile::new()?;
    for candidate in candidates {
        writeln!(probe, "{candidate} {PROBE_PUBLIC_KEY}")?;
    }
    probe.as_file().flush()?;
    let matched = keys::known_hosts::known_host_keys_path(host, port, probe.path())?;
    Ok(matched
        .into_iter()
        .filter_map(|(line_num, _)| candidates.get(line_num.saturating_sub(1)).copied())
        .map(str::to_owned)
        .collect())
}

fn rotate_known_hosts(
    previous_text: &str,
    host: &str,
    port: u16,
    algorithm: &str,
    openssh_key: &str,
) -> anyhow::Result<String> {
    let authority = known_hosts_authority(host, port);
    let mut seen_hashed = HashSet::new();
    let mut hashed_candidates = Vec::new();
    for line in previous_text.lines() {
        if let Some((hosts, line_algo, _)) = parse_known_hosts_line(line)
            && line_algo == algorithm
        {
            for entry in hosts.split(',') {
                if entry.starts_with("|1|") && seen_hashed.insert(entry) {
                    hashed_candidates.push(entry);
                }
            }
        }
    }
    let matched_hashed = match_hashed_hosts(host, port, &hashed_candidates)?;

    let mut kept = Vec::new();
    for line in previous_text.lines() {
        if line.trim().is_empty() {
            if !kept.is_empty() {
                kept.push(String::new());
            }
            continue;
        }
        let Some((hosts, line_algo, remainder)) = parse_known_hosts_line(line) else {
            kept.push(line.to_owned());
            continue;
        };
        if line_algo != algorithm {
            kept.push(line.to_owned());
            continue;
        }
        let mut removed_any = false;
        let mut remaining_hosts = Vec::new();
        for entry in hosts.split(',') {
            let matches = if entry.starts_with("|1|") {
                matched_hashed.contains(entry)
            } else {
                entry == authority.as_ref()
            };
            if matches {
                removed_any = true;
            } else if !entry.is_empty() {
                remaining_hosts.push(entry);
            }
        }
        if !removed_any {
            kept.push(line.to_owned());
        } else if !remaining_hosts.is_empty() {
            kept.push(format!("{} {remainder}", remaining_hosts.join(",")));
        }
    }
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }

    let mut output = String::new();
    for line in kept {
        output.push_str(&line);
        output.push('\n');
    }
    output.push_str(&format!("{authority} {}\n", openssh_key.trim()));
    Ok(output)
}

impl Verifier {
    async fn record_known_host(&self, key: &keys::PublicKey) -> anyhow::Result<()> {
        let known_hosts = self.known_hosts.clone();
        let host = self.host.clone();
        let port = self.port;
        let algorithm = key.algorithm().as_str().to_owned();
        let openssh_key = key.to_openssh()?;
        tokio::task::spawn_blocking(move || {
            Self::write_known_host(&known_hosts, &host, port, &algorithm, &openssh_key)
        })
        .await?
    }

    fn write_known_host(
        known_hosts: &Path,
        host: &str,
        port: u16,
        algorithm: &str,
        openssh_key: &str,
    ) -> anyhow::Result<()> {
        let store = crate::persistence::File::new(known_hosts.to_path_buf());
        let previous = store.read()?;
        let previous_text = std::str::from_utf8(previous.as_deref().unwrap_or_default())?;
        let text = rotate_known_hosts(previous_text, host, port, algorithm, openssh_key)?;
        store.write(previous.as_deref(), text.as_bytes())?;
        Ok(())
    }
}

impl client::Handler for Verifier {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        offered: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = offered.public_key();
        let host = self.host.clone();
        let port = self.port;
        let known_hosts = self.known_hosts.clone();
        let known = tokio::task::spawn_blocking(move || {
            keys::known_hosts::known_host_keys_path(&host, port, &known_hosts)
        })
        .await??;
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
        self.record_known_host(&key).await?;
        Ok(true)
    }
}

fn client_config() -> Arc<client::Config> {
    Arc::new(client::Config {
        nodelay: true,
        inactivity_timeout: Some(SSH_INACTIVITY_TIMEOUT),
        keepalive_interval: Some(SSH_KEEPALIVE_INTERVAL),
        keepalive_max: SSH_KEEPALIVE_MAX,
        ..Default::default()
    })
}

async fn connect_with_timeout<T, E: Into<anyhow::Error>>(
    timeout: Duration,
    connect: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, Error> {
    tokio::time::timeout(timeout, connect)
        .await
        .map_err(|elapsed| Error::protocol("SSH connection timeout", elapsed))?
        .map_err(|e| Error::protocol("SSH connection", e))
}

#[cfg(unix)]
async fn authenticate_with_agent(
    ssh: &mut client::Handle<Verifier>,
    user: &str,
) -> Result<bool, Error> {
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
            .authenticate_publickey_with(user, identity.public_key().into_owned(), hash, &mut agent)
            .await
            .map_err(|e| Error::protocol("SSH agent authentication", e))?
            .success()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(unix)]
fn resolve_agent_outcome(
    agent_result: Result<bool, Error>,
    has_password: bool,
) -> Result<bool, Error> {
    match agent_result {
        Ok(matched) => Ok(matched),
        Err(_) if has_password => Ok(false),
        Err(err) => Err(err),
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
        let config = client_config();
        let mut ssh = connect_with_timeout(
            SSH_CONNECT_TIMEOUT,
            client::connect(config, (address.host(), address.port()), verifier),
        )
        .await?;
        let user = address.username();
        let credentials = options.credentials;
        let mut authenticated = false;
        if let Some(path) = credentials.private_key {
            let passphrase = credentials.key_passphrase;
            let key = tokio::task::spawn_blocking(move || {
                keys::load_secret_key(path, passphrase.as_deref())
            })
            .await
            .map_err(|e| Error::protocol("SSH key", e))?
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
        if !authenticated && std::env::var_os("SSH_AUTH_SOCK").is_some() {
            authenticated = resolve_agent_outcome(
                authenticate_with_agent(&mut ssh, &user).await,
                !credentials.password.is_empty(),
            )?;
        }
        if !authenticated && !credentials.password.is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_ED25519_KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
    const ALT_ED25519_KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILagOJFgwaMNhBWQINinKOXmqS4Gh5NgxgriXwdOoINJ";
    const SAMPLE_ECDSA_KEY: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBMxBTpMIGvo7CnordO7wP0QQRqpBwUjOLl4eMhfucfE1sjTYyK5wmTl1UqoSDS1PtRVTBdl+0+9pquFb46U7fwg=";
    const SAMPLE_RSA_KEY: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAACAQC0Xtz3tSNgbUQAXem4d+d6hMx7S8Nwm/DOO2AWyWCru+n/+jQ7wz2b5+3oG2+7GbWZNGj8HCc6wJSA3jUsgv1N6PImIWclD14qvoqY3Dea1J0CJgXnnM1xKzBz9C6pDHGvdtySg+yzEO41Xt4u7HFn4Zx5SGuI2NBsF5mtMLZXSi33jCIWVIkrJVd7sZaY8jiqeVZBB/UvkLPWewGVuSXZHT84pNw4+S0Rh6P6zdNutK+JbeuO+5Bav4h9iw4t2sdRkEiWg/AdMoSKmo97Gigq2mKdW12ivnXxz3VfxrCgYJj9WwaUUWSfnAju5SiNly0cTEAN4dJ7yB0mfLKope1kRhPsNaOuUmMUqlu/hBDM/luOCzNjyVJ+0LLB7SV5vOiV7xkVd4KbEGKou8eeCR3yjFazUe/D1pjYPssPL8cJhTSuMc+/UC9zD8yeEZhB9V+vW4NMUR+lh5+XeOzenl65lWYd/nBZXLBbpUMf1AOfbz65xluwCxr2D2lj46iApSIpvE63i3LzFkbGl9GdUiuZJLMFJzOWdhGGc97cB5OVyf8umZLqMHjaImxHEHrnPh1MOVpv87HYJtSBEsN4/omINCMZrk++CRYAIRKRpPKFWV7NQHcvw3m7XLR3KaTYe+0/MINIZwGdou9fLUU3zSd521vDjA/weasH0CyDHq7sZw==";
    // OpenSSH HMAC-SHA1 hashed entry for `example.com` on port 22.
    const EXAMPLE_COM_HASH: &str = "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak=";

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn known_hosts_lookup_and_record_run_off_runtime_workers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let known_hosts = dir.path().join("known_hosts");
        std::fs::write(
            &known_hosts,
            format!("other.example {SAMPLE_ED25519_KEY}\nexample.test ssh-ed25519 old\n"),
        )
        .expect("seed known_hosts");

        let key = keys::PublicKey::from_openssh(SAMPLE_ED25519_KEY).expect("parse sample key");
        let (tx, _rx) = mpsc::channel(1);
        let verifier = Verifier {
            host: "example.test".into(),
            port: 22,
            known_hosts: known_hosts.clone(),
            challenges: tx.clone(),
        };
        verifier
            .record_known_host(&key)
            .await
            .expect("record default port key");

        let custom_port = Verifier {
            host: "example.test".into(),
            port: 2222,
            known_hosts: known_hosts.clone(),
            challenges: tx,
        };
        custom_port
            .record_known_host(&key)
            .await
            .expect("record custom port key");

        let content = std::fs::read_to_string(&known_hosts).expect("read known_hosts");
        assert!(content.contains("other.example"));
        assert!(!content.contains("old"));
        assert!(content.contains(&format!("example.test {SAMPLE_ED25519_KEY}")));
        assert!(content.contains(&format!("[example.test]:2222 {SAMPLE_ED25519_KEY}")));
        assert!(!content.starts_with('\n'));
        assert!(content.ends_with('\n'));
        assert!(!content.ends_with("\n\n"));

        let lookup_path = known_hosts.clone();
        let known = tokio::task::spawn_blocking(move || {
            keys::known_hosts::known_host_keys_path("example.test", 22, &lookup_path)
        })
        .await
        .expect("join lookup")
        .expect("parse known_hosts");
        assert_eq!(known.len(), 1);
    }

    #[test]
    fn known_hosts_empty_file_has_no_leading_blank_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("missing_known_hosts");
        Verifier::write_known_host(
            &missing,
            "example.test",
            22,
            "ssh-ed25519",
            SAMPLE_ED25519_KEY,
        )
        .expect("create known_hosts");
        let created = std::fs::read_to_string(&missing).expect("read created");
        assert_eq!(created, format!("example.test {SAMPLE_ED25519_KEY}\n"));

        let blank = dir.path().join("blank_known_hosts");
        std::fs::write(&blank, "\n\n").expect("seed blank known_hosts");
        Verifier::write_known_host(
            &blank,
            "example.test",
            22,
            "ssh-ed25519",
            SAMPLE_ED25519_KEY,
        )
        .expect("replace blank known_hosts");
        let replaced = std::fs::read_to_string(&blank).expect("read replaced");
        assert_eq!(replaced, format!("example.test {SAMPLE_ED25519_KEY}\n"));
    }

    #[test]
    fn known_hosts_rotates_hashed_entry_only_for_matching_algorithm() {
        let dir = tempfile::tempdir().expect("tempdir");
        let known_hosts = dir.path().join("known_hosts");
        std::fs::write(
            &known_hosts,
            format!(
                "{EXAMPLE_COM_HASH} {ALT_ED25519_KEY}\n{EXAMPLE_COM_HASH} {SAMPLE_ECDSA_KEY}\n"
            ),
        )
        .expect("seed hashed known_hosts");

        Verifier::write_known_host(
            &known_hosts,
            "example.com",
            22,
            "ssh-ed25519",
            SAMPLE_ED25519_KEY,
        )
        .expect("rotate hashed ed25519 key");

        let content = std::fs::read_to_string(&known_hosts).expect("read known_hosts");
        assert!(!content.contains(ALT_ED25519_KEY));
        assert!(content.contains(&format!("{EXAMPLE_COM_HASH} {SAMPLE_ECDSA_KEY}")));
        assert!(content.contains(&format!("example.com {SAMPLE_ED25519_KEY}")));
        assert!(!content.starts_with('\n'));
        assert!(content.ends_with('\n'));
        assert!(!content.ends_with("\n\n"));

        let known = keys::known_hosts::known_host_keys_path("example.com", 22, &known_hosts)
            .expect("parse rotated known_hosts");
        assert_eq!(known.len(), 2);
        assert!(known.iter().any(|(_, key)| {
            key == &keys::PublicKey::from_openssh(SAMPLE_ECDSA_KEY).expect("ecdsa")
        }));
        assert!(known.iter().any(|(_, key)| {
            key == &keys::PublicKey::from_openssh(SAMPLE_ED25519_KEY).expect("ed25519")
        }));
    }

    #[test]
    fn known_hosts_rotates_comma_separated_hosts_preserving_other_hosts_and_algorithms() {
        let dir = tempfile::tempdir().expect("tempdir");
        let known_hosts = dir.path().join("known_hosts");
        std::fs::write(
            &known_hosts,
            format!(
                "# comment\n\
                 example.com,alias.example {ALT_ED25519_KEY} host-comment\n\
                 [example.com]:2222,other.example {ALT_ED25519_KEY}\n\
                 example.com {SAMPLE_RSA_KEY}\n\
                 @revoked example.com {ALT_ED25519_KEY}\n"
            ),
        )
        .expect("seed comma-separated known_hosts");

        Verifier::write_known_host(
            &known_hosts,
            "example.com",
            22,
            "ssh-ed25519",
            SAMPLE_ED25519_KEY,
        )
        .expect("rotate default port ed25519");

        let content = std::fs::read_to_string(&known_hosts).expect("read known_hosts");
        assert!(content.contains("# comment\n"));
        assert!(content.contains(&format!("alias.example {ALT_ED25519_KEY} host-comment\n")));
        assert!(content.contains(&format!(
            "[example.com]:2222,other.example {ALT_ED25519_KEY}\n"
        )));
        assert!(content.contains(&format!("example.com {SAMPLE_RSA_KEY}\n")));
        assert!(content.contains(&format!("@revoked example.com {ALT_ED25519_KEY}\n")));
        assert!(content.ends_with(&format!("example.com {SAMPLE_ED25519_KEY}\n")));
        assert!(!content.contains("example.com,alias.example"));

        Verifier::write_known_host(
            &known_hosts,
            "example.com",
            2222,
            "ssh-ed25519",
            SAMPLE_ED25519_KEY,
        )
        .expect("rotate custom port ed25519");
        let updated = std::fs::read_to_string(&known_hosts).expect("read updated known_hosts");
        assert!(updated.contains(&format!("other.example {ALT_ED25519_KEY}\n")));
        assert!(!updated.contains("[example.com]:2222,other.example"));
        assert!(updated.ends_with(&format!("[example.com]:2222 {SAMPLE_ED25519_KEY}\n")));
    }

    #[tokio::test]
    async fn ssh_config_sets_keepalive_and_connect_timeout_maps_errors() {
        let config = client_config();
        assert!(config.nodelay);
        assert_eq!(config.inactivity_timeout, Some(Duration::from_secs(60)));
        assert_eq!(config.keepalive_interval, Some(Duration::from_secs(15)));
        assert_eq!(config.keepalive_max, 3);

        let timed_out = connect_with_timeout(
            Duration::from_millis(10),
            std::future::pending::<Result<(), anyhow::Error>>(),
        )
        .await;
        assert!(matches!(
            timed_out,
            Err(Error::Protocol {
                operation: "SSH connection timeout",
                ..
            })
        ));

        let failed = connect_with_timeout(Duration::from_secs(1), async {
            Err::<(), _>(anyhow::anyhow!("refused"))
        })
        .await;
        assert!(matches!(
            failed,
            Err(Error::Protocol {
                operation: "SSH connection",
                ..
            })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn agent_error_falls_back_only_when_password_is_present() {
        let err = || Error::protocol("SSH agent", anyhow::anyhow!("stale socket"));
        assert!(matches!(resolve_agent_outcome(Err(err()), true), Ok(false)));
        assert!(matches!(
            resolve_agent_outcome(Err(err()), false),
            Err(Error::Protocol {
                operation: "SSH agent",
                ..
            })
        ));
        assert!(matches!(resolve_agent_outcome(Ok(false), false), Ok(false)));
        assert!(matches!(resolve_agent_outcome(Ok(true), false), Ok(true)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn load_secret_key_missing_file_reports_protocol_error_off_worker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("id_missing");
        let result = tokio::task::spawn_blocking(move || keys::load_secret_key(missing, None))
            .await
            .map_err(|e| Error::protocol("SSH key", e))
            .and_then(|res| res.map_err(|e| Error::protocol("SSH key", e)));
        assert!(matches!(
            result,
            Err(Error::Protocol {
                operation: "SSH key",
                ..
            })
        ));
    }
}
