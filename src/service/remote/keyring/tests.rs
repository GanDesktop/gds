use super::*;
use std::path::Path;

#[test]
fn attribute_encoding_and_decoding_cover_password_key_guest_and_isolation() {
    let sftp_default = Address::parse("sftp://alice@files.example.com/srv/data").expect("sftp");
    let sftp_sub = Address::parse("sftp://alice@files.example.com/home/alice").expect("sftp sub");
    let sftp_port = Address::parse("sftp://alice@files.example.com:2222/srv/data").expect("port");
    let sftp_other_user = Address::parse("sftp://bob@files.example.com/srv/data").expect("bob");
    let smb_domain = Address::parse("smb://CORP%5Calice@nas.local/media/photos").expect("smb");
    let ipv6 = Address::parse("sftp://alice@[2001:db8::1]:2200/").expect("ipv6");

    // Different folders on the same connection share lookup attributes.
    assert_eq!(
        lookup_attributes(&sftp_default),
        lookup_attributes(&sftp_sub)
    );
    // Different ports, users, or protocols are isolated.
    assert_ne!(
        lookup_attributes(&sftp_default),
        lookup_attributes(&sftp_port)
    );
    assert_ne!(
        lookup_attributes(&sftp_default),
        lookup_attributes(&sftp_other_user)
    );
    assert_ne!(
        lookup_attributes(&sftp_default),
        lookup_attributes(&smb_domain)
    );

    assert_eq!(
        item_label(&sftp_default),
        "GDS Files (SFTP): alice@files.example.com"
    );
    assert_eq!(
        item_label(&sftp_port),
        "GDS Files (SFTP): alice@files.example.com:2222"
    );
    assert_eq!(
        item_label(&smb_domain),
        r"GDS Files (SMB): CORP\alice@nas.local"
    );
    assert_eq!(
        item_label(&ipv6),
        "GDS Files (SFTP): alice@[2001:db8::1]:2200"
    );

    // Password round-trip.
    let password_creds = Credentials {
        password: "s3cr3t-password".to_owned(),
        ..Default::default()
    };
    let encoded = encode_item(&sftp_default, &password_creds)
        .expect("encode password")
        .expect("non-empty item");
    assert_eq!(encoded.attributes[ATTR_SCHEMA], SCHEMA_NAME);
    assert_eq!(encoded.attributes[ATTR_APPLICATION], APPLICATION_ID);
    assert_eq!(encoded.attributes[ATTR_PROTOCOL], "sftp");
    assert_eq!(encoded.attributes[ATTR_PORT], "22");
    assert_eq!(encoded.attributes[ATTR_SECRET_KIND], SECRET_KIND_PASSWORD);

    let as_hash: HashMap<String, String> = encoded.attributes.into_iter().collect();
    let decoded = decode_credentials(&as_hash, encoded.secret.as_bytes()).expect("decode password");
    assert_eq!(decoded.password, "s3cr3t-password");
    assert!(decoded.private_key.is_none());
    assert!(decoded.key_passphrase.is_none());

    // Private key with passphrase round-trip.
    let key_creds = Credentials {
        private_key: Some(PathBuf::from("/home/alice/.ssh/id_ed25519")),
        key_passphrase: Some("pass-phrase".to_owned()),
        ..Default::default()
    };
    let encoded = encode_item(&sftp_default, &key_creds)
        .expect("encode key")
        .expect("non-empty item");
    assert_eq!(
        encoded.attributes[ATTR_SECRET_KIND],
        SECRET_KIND_PRIVATE_KEY
    );
    assert_eq!(
        encoded.attributes[ATTR_PRIVATE_KEY_PATH],
        "/home/alice/.ssh/id_ed25519"
    );
    let as_hash: HashMap<String, String> = encoded.attributes.into_iter().collect();
    let decoded = decode_credentials(&as_hash, encoded.secret.as_bytes()).expect("decode key");
    assert!(decoded.password.is_empty());
    assert_eq!(
        decoded.private_key.as_deref(),
        Some(Path::new("/home/alice/.ssh/id_ed25519"))
    );
    assert_eq!(decoded.key_passphrase.as_deref(), Some("pass-phrase"));

    // Private key without passphrase round-trip.
    let unencrypted_key = Credentials {
        private_key: Some(PathBuf::from("/home/alice/.ssh/id_rsa")),
        key_passphrase: None,
        ..Default::default()
    };
    let encoded = encode_item(&sftp_default, &unencrypted_key)
        .expect("encode key without passphrase")
        .expect("non-empty item");
    let as_hash: HashMap<String, String> = encoded.attributes.into_iter().collect();
    let decoded =
        decode_credentials(&as_hash, encoded.secret.as_bytes()).expect("decode unencrypted key");
    assert_eq!(
        decoded.private_key.as_deref(),
        Some(Path::new("/home/alice/.ssh/id_rsa"))
    );
    assert!(decoded.key_passphrase.is_none());

    // Empty credentials (SSH agent / SMB guest) produce no item.
    assert!(
        encode_item(&sftp_default, &Credentials::default())
            .expect("empty credentials")
            .is_none()
    );
    assert!(!Credentials::default().has_secret());
    assert!(password_creds.has_secret());
    assert!(unencrypted_key.has_secret());

    // Relative private key paths are rejected.
    let relative = Credentials {
        private_key: Some(PathBuf::from(".ssh/id_ed25519")),
        ..Default::default()
    };
    assert!(encode_item(&sftp_default, &relative).is_err());
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod dbus_integration {
    use std::{
        collections::HashMap,
        io::{BufRead as _, BufReader},
        path::PathBuf,
        process::{Command as Process, Stdio},
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
        },
    };

    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Str, Value};

    use super::super::*;

    const SERVICE_BUS_NAME: &str = "org.freedesktop.secrets";
    const SERVICE_PATH: &str = "/org/freedesktop/secrets";
    const COLLECTION_PATH: &str = "/org/freedesktop/secrets/collection/default";

    #[derive(Clone)]
    struct StoredItem {
        path: OwnedObjectPath,
        attributes: HashMap<String, String>,
        secret: Vec<u8>,
        content_type: String,
        modified: u64,
        locked: bool,
    }

    #[derive(Default)]
    struct MockStore {
        has_default: bool,
        collection_locked: bool,
        next_session: AtomicU64,
        next_item: AtomicU64,
        clock: AtomicU64,
        items: HashMap<String, StoredItem>,
    }

    struct MockService {
        connection: zbus::Connection,
        store: Arc<Mutex<MockStore>>,
    }

    #[zbus::interface(name = "org.freedesktop.Secret.Service")]
    impl MockService {
        async fn open_session(
            &self,
            algorithm: &str,
            _input: Value<'_>,
        ) -> zbus::fdo::Result<(OwnedValue, OwnedObjectPath)> {
            if algorithm != "plain" {
                return Err(zbus::fdo::Error::NotSupported(
                    "mock secret service supports plain sessions only".to_owned(),
                ));
            }
            let id = self
                .store
                .lock()
                .expect("store")
                .next_session
                .fetch_add(1, Ordering::Relaxed)
                + 1;
            let path = OwnedObjectPath::try_from(format!("/org/freedesktop/secrets/session/s{id}"))
                .expect("session path");
            self.connection
                .object_server()
                .at(path.clone(), MockSession)
                .await
                .map_err(|err| zbus::fdo::Error::Failed(err.to_string()))?;
            let output = OwnedValue::try_from(Value::from(Str::from(""))).expect("empty output");
            Ok((output, path))
        }

        async fn read_alias(&self, name: &str) -> OwnedObjectPath {
            let has_default = self.store.lock().expect("store").has_default;
            if name == "default" && has_default {
                OwnedObjectPath::try_from(COLLECTION_PATH).expect("collection path")
            } else {
                OwnedObjectPath::try_from("/").expect("root path")
            }
        }

        async fn create_collection(
            &self,
            _properties: HashMap<String, OwnedValue>,
            alias: &str,
        ) -> zbus::fdo::Result<(OwnedObjectPath, OwnedObjectPath)> {
            if alias == "default" {
                let first = {
                    let mut guard = self.store.lock().expect("store");
                    let first = !guard.has_default;
                    guard.has_default = true;
                    first
                };
                if first {
                    self.connection
                        .object_server()
                        .at(
                            COLLECTION_PATH,
                            MockCollection {
                                connection: self.connection.clone(),
                                store: Arc::clone(&self.store),
                            },
                        )
                        .await
                        .map_err(|err| zbus::fdo::Error::Failed(err.to_string()))?;
                }
            }
            Ok((
                OwnedObjectPath::try_from(COLLECTION_PATH).expect("collection path"),
                OwnedObjectPath::try_from("/").expect("no prompt"),
            ))
        }

        async fn unlock(
            &self,
            objects: Vec<OwnedObjectPath>,
        ) -> (Vec<OwnedObjectPath>, OwnedObjectPath) {
            let mut guard = self.store.lock().expect("store");
            for path in &objects {
                if path.as_str() == COLLECTION_PATH {
                    guard.collection_locked = false;
                    for item in guard.items.values_mut() {
                        item.locked = false;
                    }
                } else if let Some(item) = guard.items.get_mut(path.as_str()) {
                    item.locked = false;
                }
            }
            (objects, OwnedObjectPath::try_from("/").expect("no prompt"))
        }
    }

    struct MockSession;

    #[zbus::interface(name = "org.freedesktop.Secret.Session")]
    impl MockSession {
        async fn close(&self) {}
    }

    struct MockCollection {
        connection: zbus::Connection,
        store: Arc<Mutex<MockStore>>,
    }

    #[zbus::interface(name = "org.freedesktop.Secret.Collection")]
    impl MockCollection {
        #[zbus(property)]
        async fn locked(&self) -> bool {
            self.store.lock().expect("store").collection_locked
        }

        async fn search_items(&self, attributes: HashMap<String, String>) -> Vec<OwnedObjectPath> {
            let guard = self.store.lock().expect("store");
            guard
                .items
                .values()
                .filter(|item| {
                    attributes
                        .iter()
                        .all(|(k, v)| item.attributes.get(k) == Some(v))
                })
                .map(|item| item.path.clone())
                .collect()
        }

        async fn create_item(
            &self,
            properties: HashMap<String, OwnedValue>,
            secret: (OwnedObjectPath, Vec<u8>, Vec<u8>, String),
            replace: bool,
        ) -> zbus::fdo::Result<(OwnedObjectPath, OwnedObjectPath)> {
            let attributes = properties
                .get("org.freedesktop.Secret.Item.Attributes")
                .and_then(|value| HashMap::<String, String>::try_from(value.try_clone().ok()?).ok())
                .unwrap_or_default();

            let (item_path, newly_created) = {
                let mut guard = self.store.lock().expect("store");
                let now = guard.clock.fetch_add(1, Ordering::Relaxed) + 1;
                let existing_key = replace.then(|| {
                    guard
                        .items
                        .iter()
                        .find(|(_, item)| item.attributes == attributes)
                        .map(|(k, _)| k.clone())
                });
                if let Some(Some(existing_key)) = existing_key {
                    let entry = guard.items.get_mut(&existing_key).expect("existing item");
                    entry.secret = secret.2;
                    entry.content_type = secret.3;
                    entry.modified = now;
                    entry.locked = false;
                    (entry.path.clone(), false)
                } else {
                    let id = guard.next_item.fetch_add(1, Ordering::Relaxed) + 1;
                    let path = OwnedObjectPath::try_from(format!("{COLLECTION_PATH}/item_{id}"))
                        .expect("item path");
                    let key = path.as_str().to_owned();
                    guard.items.insert(
                        key.clone(),
                        StoredItem {
                            path: path.clone(),
                            attributes,
                            secret: secret.2,
                            content_type: secret.3,
                            modified: now,
                            locked: false,
                        },
                    );
                    (path, true)
                }
            };

            if newly_created {
                self.connection
                    .object_server()
                    .at(
                        item_path.clone(),
                        MockItem {
                            path: item_path.as_str().to_owned(),
                            store: Arc::clone(&self.store),
                        },
                    )
                    .await
                    .map_err(|err| zbus::fdo::Error::Failed(err.to_string()))?;
            }

            Ok((
                item_path,
                OwnedObjectPath::try_from("/").expect("no prompt"),
            ))
        }
    }

    struct MockItem {
        path: String,
        store: Arc<Mutex<MockStore>>,
    }

    #[zbus::interface(name = "org.freedesktop.Secret.Item")]
    impl MockItem {
        #[zbus(property)]
        async fn locked(&self) -> bool {
            self.store
                .lock()
                .expect("store")
                .items
                .get(&self.path)
                .is_some_and(|item| item.locked)
        }

        #[zbus(property)]
        async fn modified(&self) -> u64 {
            self.store
                .lock()
                .expect("store")
                .items
                .get(&self.path)
                .map_or(0, |item| item.modified)
        }

        #[zbus(property)]
        async fn attributes(&self) -> HashMap<String, String> {
            self.store
                .lock()
                .expect("store")
                .items
                .get(&self.path)
                .map(|item| item.attributes.clone())
                .unwrap_or_default()
        }

        async fn get_secret(
            &self,
            session: OwnedObjectPath,
        ) -> zbus::fdo::Result<(OwnedObjectPath, Vec<u8>, Vec<u8>, String)> {
            let guard = self.store.lock().expect("store");
            let Some(item) = guard.items.get(&self.path) else {
                return Err(zbus::fdo::Error::Failed("item deleted".to_owned()));
            };
            Ok((
                session,
                Vec::new(),
                item.secret.clone(),
                item.content_type.clone(),
            ))
        }

        async fn delete(&self) -> OwnedObjectPath {
            self.store.lock().expect("store").items.remove(&self.path);
            OwnedObjectPath::try_from("/").expect("no prompt")
        }
    }

    #[test]
    fn private_bus_handles_unavailable_store_lookup_unlock_replace_and_delete() {
        if std::env::var_os("GDS_REMOTE_KEYRING_TEST").is_none() {
            let root = tempfile::tempdir().expect("bus directory");
            let config = root.path().join("bus.conf");
            let listen = format!("unix:tmpdir={}", root.path().display());
            std::fs::write(
                &config,
                format!(
                    "<busconfig><type>session</type><listen>{listen}</listen><auth>EXTERNAL</auth><policy context=\"default\"><allow send_destination=\"*\"/><allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>"
                ),
            )
            .expect("private bus config");
            let mut bus = Process::new("dbus-daemon")
                .args(["--nofork", "--print-address=1", "--config-file"])
                .arg(&config)
                .stdout(Stdio::piped())
                .spawn()
                .expect("private dbus-daemon");
            let mut address = String::new();
            BufReader::new(bus.stdout.take().expect("bus stdout"))
                .read_line(&mut address)
                .expect("bus address");

            let result = Process::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "service::remote::keyring::tests::dbus_integration::private_bus_handles_unavailable_store_lookup_unlock_replace_and_delete",
                    "--nocapture",
                ])
                .env("GDS_REMOTE_KEYRING_TEST", "1")
                .env("DBUS_SESSION_BUS_ADDRESS", address.trim())
                .output()
                .expect("isolated keyring test");

            let _ = bus.kill();
            let _ = bus.wait();
            assert!(
                result.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
            return;
        }

        tokio::runtime::Runtime::new()
            .expect("Tokio")
            .block_on(async {
                let sftp_srv = Address::parse("sftp://alice@example.com/srv").expect("sftp");
                let sftp_home = Address::parse("sftp://alice@example.com/home").expect("sftp home");
                let sftp_other = Address::parse("sftp://bob@example.com/srv").expect("bob");
                let smb_share = Address::parse("smb://CORP%5Calice@nas/media").expect("smb");

                // 1. Without any Secret Service daemon on the bus, degrade gracefully.
                assert!(!is_available().await);
                assert!(!has_saved(&sftp_srv).await.expect("has_saved unavailable"));
                assert!(
                    lookup(&sftp_srv)
                        .await
                        .expect("lookup unavailable")
                        .is_none()
                );
                assert!(!delete(&sftp_srv).await.expect("delete unavailable"));
                assert!(matches!(
                    store(
                        &sftp_srv,
                        &Credentials {
                            password: "pw".to_owned(),
                            ..Default::default()
                        }
                    )
                    .await,
                    Err(Error::Unavailable)
                ));

                // 2. Start in-process mock org.freedesktop.secrets on the private bus.
                let store_state = Arc::new(Mutex::new(MockStore::default()));
                let server = zbus::Connection::session().await.expect("server bus");
                server
                    .object_server()
                    .at(
                        SERVICE_PATH,
                        MockService {
                            connection: server.clone(),
                            store: Arc::clone(&store_state),
                        },
                    )
                    .await
                    .expect("register service");
                server
                    .request_name(SERVICE_BUS_NAME)
                    .await
                    .expect("own org.freedesktop.secrets");

                assert!(is_available().await);
                // Lookup before any default collection exists must not create one.
                assert!(lookup(&sftp_srv).await.expect("lookup empty").is_none());
                assert!(!store_state.lock().expect("store").has_default);

                // 3. Store SFTP password, verify lookup across folders & isolation.
                store(
                    &sftp_srv,
                    &Credentials {
                        password: "initial-password".to_owned(),
                        ..Default::default()
                    },
                )
                .await
                .expect("store password");
                assert!(has_saved(&sftp_home).await.expect("has_saved"));
                let fetched = lookup(&sftp_home)
                    .await
                    .expect("lookup password")
                    .expect("saved password");
                assert_eq!(fetched.password, "initial-password");
                assert!(lookup(&sftp_other).await.expect("isolated user").is_none());
                assert!(
                    lookup(&smb_share)
                        .await
                        .expect("isolated protocol")
                        .is_none()
                );

                // 4. Locking the item causes lookup() to unlock it transparently.
                {
                    let mut guard = store_state.lock().expect("store");
                    for item in guard.items.values_mut() {
                        item.locked = true;
                    }
                }
                let unlocked = lookup(&sftp_srv)
                    .await
                    .expect("lookup locked")
                    .expect("unlocked password");
                assert_eq!(unlocked.password, "initial-password");

                // 5. Switching from password to private key replaces the item cleanly without orphans.
                store(
                    &sftp_srv,
                    &Credentials {
                        private_key: Some(PathBuf::from("/home/alice/.ssh/id_ed25519")),
                        key_passphrase: Some("key-secret".to_owned()),
                        ..Default::default()
                    },
                )
                .await
                .expect("store private key");
                assert_eq!(store_state.lock().expect("store").items.len(), 1);
                let key_creds = lookup(&sftp_srv)
                    .await
                    .expect("lookup key")
                    .expect("saved key");
                assert!(key_creds.password.is_empty());
                assert_eq!(
                    key_creds.private_key,
                    Some(PathBuf::from("/home/alice/.ssh/id_ed25519"))
                );
                assert_eq!(key_creds.key_passphrase.as_deref(), Some("key-secret"));

                // 5b. Unencrypted private key (empty passphrase) round-trips with key_passphrase = None.
                store(
                    &sftp_srv,
                    &Credentials {
                        private_key: Some(PathBuf::from("/home/alice/.ssh/id_ed25519")),
                        key_passphrase: None,
                        ..Default::default()
                    },
                )
                .await
                .expect("store unencrypted key");
                assert_eq!(store_state.lock().expect("store").items.len(), 1);
                let unencrypted_creds = lookup(&sftp_srv)
                    .await
                    .expect("lookup unencrypted key")
                    .expect("saved unencrypted key");
                assert_eq!(
                    unencrypted_creds.private_key,
                    Some(PathBuf::from("/home/alice/.ssh/id_ed25519"))
                );
                assert!(unencrypted_creds.key_passphrase.is_none());

                // 6. Store SMB password separately without touching SFTP item.
                store(
                    &smb_share,
                    &Credentials {
                        password: "smb-password".to_owned(),
                        ..Default::default()
                    },
                )
                .await
                .expect("store smb");
                assert_eq!(store_state.lock().expect("store").items.len(), 2);
                assert_eq!(
                    lookup(&smb_share)
                        .await
                        .expect("lookup smb")
                        .expect("smb creds")
                        .password,
                    "smb-password"
                );

                // 7. Delete SFTP credentials; SMB remains intact; second delete returns false.
                assert!(delete(&sftp_srv).await.expect("delete sftp"));
                assert!(!delete(&sftp_srv).await.expect("delete sftp again"));
                assert!(
                    lookup(&sftp_srv)
                        .await
                        .expect("lookup after delete")
                        .is_none()
                );
                assert!(has_saved(&smb_share).await.expect("smb still saved"));

                drop(server);
            });
    }
}
