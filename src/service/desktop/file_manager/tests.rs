use super::*;
const APP_BUS_NAME: &str = "io.github.GanDesktop.GdsFiles";
use std::{
    io::{BufRead as _, BufReader},
    process::{Command as Process, Stdio},
};

#[test]
fn local_uri_parsing_preserves_names_directories_and_root() {
    use std::os::unix::ffi::OsStringExt as _;
    let raw = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/a #\xff".to_vec()));
    let uris = [
        raw.clone(),
        PathBuf::from("/tmp/folder"),
        PathBuf::from("/"),
    ]
    .iter()
    .map(|path| {
        url::Url::from_file_path(path)
            .expect("local URI")
            .to_string()
    })
    .collect::<Vec<_>>();
    assert_eq!(
        local_paths(&uris).expect("local paths"),
        [raw, PathBuf::from("/tmp/folder"), PathBuf::from("/")]
    );
    assert!(local_path("smb://server/share").is_err());
    assert!(local_path("file://remote/tmp").is_err());
}

#[test]
fn private_bus_dispatches_all_methods_and_respects_other_providers() {
    if std::env::var_os("GDS_FILE_MANAGER_TEST").is_none() {
        let root = tempfile::tempdir().expect("bus directory");
        let config = root.path().join("bus.conf");
        std::fs::write(&config, r#"<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>"#).expect("private bus configuration");
        let mut bus = Process::new("dbus-daemon")
            .args(["--nofork", "--print-address=1", "--config-file"])
            .arg(config)
            .stdout(Stdio::piped())
            .spawn()
            .expect("private D-Bus");
        let mut address = String::new();
        BufReader::new(bus.stdout.take().expect("bus stdout"))
            .read_line(&mut address)
            .expect("bus address");
        let result = Process::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "service::desktop::file_manager::tests::private_bus_dispatches_all_methods_and_respects_other_providers", "--nocapture"])
            .env("GDS_FILE_MANAGER_TEST", "1").env("DBUS_SESSION_BUS_ADDRESS", address.trim()).output().expect("isolated test");
        bus.kill().expect("stop private bus");
        bus.wait().expect("reap private bus");
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
            let (sender, mut receiver) = mpsc::channel(8);
            let server = Server::connect(APP_BUS_NAME, sender.clone())
                .await
                .expect("Files service");
            assert!(server.provider_conflict.is_none());
            assert!(matches!(
                Server::connect(APP_BUS_NAME, sender.clone()).await,
                Err(Error::AlreadyRunning)
            ));
            let client = Connection::session().await.expect("client");
            let paths = vec![PathBuf::from("/tmp/a name"), PathBuf::from("/tmp/another")];
            for (method, request) in [
                (
                    "ShowFolders",
                    Request::ShowFolders {
                        directories: paths.clone(),
                    },
                ),
                (
                    "ShowItems",
                    Request::ShowItems {
                        paths: paths.clone(),
                    },
                ),
                ("ShowItemProperties", Request::ShowProperties { paths }),
            ] {
                client
                    .call_method(
                        Some(BUS_NAME),
                        OBJECT_PATH,
                        Some(BUS_NAME),
                        method,
                        &(
                            vec!["file:///tmp/a%20name", "file:///tmp/another"],
                            "caller-token",
                        ),
                    )
                    .await
                    .expect("standard call");
                let command = receiver.recv().await.expect("delivered request");
                assert_eq!(command.request, request);
                assert_eq!(command.startup_id, "caller-token");
            }
            let invalid = client
                .call_method(
                    Some(BUS_NAME),
                    OBJECT_PATH,
                    Some(BUS_NAME),
                    "ShowItems",
                    &(vec!["https://example.org/file"], ""),
                )
                .await;
            assert!(invalid.is_err());
            server.connection.close().await.expect("release provider");
            client
                .request_name(BUS_NAME)
                .await
                .expect("other file manager");
            let other = Server::connect(APP_BUS_NAME, sender)
                .await
                .expect("Files can coexist");
            assert!(other.provider_conflict.is_some());
            Server::open_existing(APP_BUS_NAME, std::path::Path::new("/tmp"), "another-token")
                .await
                .expect("dispatch to Files identity");
            let request = receiver.recv().await.expect("own request");
            assert_eq!(request.startup_id, "another-token");
            assert_eq!(
                request.request,
                Request::NewWindow {
                    directory: PathBuf::from("/tmp")
                }
            );
            drop(receiver);
            let closing =
                Server::open_existing(APP_BUS_NAME, std::path::Path::new("/tmp"), "closed")
                    .await
                    .unwrap_err();
            assert!(
                closing.to_string().contains("file manager is closing"),
                "unexpected error: {closing}"
            );
            other.connection.close().await.expect("close Files");
            client.close().await.expect("close other provider");
        });
}

#[test]
fn launcher_interface_constant_and_name_has_no_owner_retry() {
    assert_eq!(
        <Launcher as zbus::object_server::Interface>::name().as_str(),
        LAUNCHER_INTERFACE
    );
    assert_eq!(
        <Interface as zbus::object_server::Interface>::name().as_str(),
        BUS_NAME
    );

    tokio::runtime::Runtime::new()
        .expect("Tokio")
        .block_on(async {
            let mut requests = 0usize;
            let mut owner_queries = 0usize;
            let resolved = resolve_provider_conflict(
                || {
                    requests += 1;
                    let attempt = requests;
                    async move {
                        if attempt == 1 {
                            Err(zbus::Error::NameTaken)
                        } else {
                            Ok(RequestNameReply::PrimaryOwner)
                        }
                    }
                },
                || {
                    owner_queries += 1;
                    async { Err(zbus::fdo::Error::NameHasNoOwner("owner exited".into())) }
                },
            )
            .await
            .expect("retry after NameHasNoOwner should claim bus name");
            assert_eq!(resolved, None);
            assert_eq!(requests, 2);
            assert_eq!(owner_queries, 1);

            let mut requests = 0usize;
            let conflict = resolve_provider_conflict(
                || {
                    requests += 1;
                    async { Err(zbus::Error::NameTaken) }
                },
                || {
                    let first = owner_queries == 1;
                    owner_queries += 1;
                    async move {
                        if first {
                            Err(zbus::fdo::Error::NameHasNoOwner("owner exited".into()))
                        } else {
                            Ok(":1.42".to_string())
                        }
                    }
                },
            )
            .await
            .expect("conflict after retry should report new owner");
            assert_eq!(conflict.as_deref(), Some(":1.42"));
            assert_eq!(requests, 2);

            let failed = resolve_provider_conflict(
                || async { Err(zbus::Error::NameTaken) },
                || async { Err(zbus::fdo::Error::NameHasNoOwner("flapping".into())) },
            )
            .await;
            assert!(matches!(failed, Err(Error::Bus(_))));
        });
}
