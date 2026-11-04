use super::*;
const APP_BUS_NAME: &str = "io.github.GanDesktop.GdsFiles";
use std::{
    io::{BufRead as _, BufReader},
    process::{Command as Process, Stdio},
};

#[test]
fn local_uri_grouping_preserves_names_directory_reveals_and_root() {
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
    let command =
        Command::from_uris(Operation::Items, &uris, "activation-token".into()).expect("reveal");
    assert_eq!(command.targets.len(), 2);
    assert_eq!(command.targets[0].directory, PathBuf::from("/tmp"));
    assert_eq!(
        command.targets[0].selection,
        [raw, PathBuf::from("/tmp/folder")]
    );
    assert_eq!(command.targets[1].directory, PathBuf::from("/"));
    assert_eq!(command.targets[1].selection, [PathBuf::from("/")]);
    assert_eq!(command.startup_id, "activation-token");
    assert!(
        Command::from_uris(
            Operation::Folders,
            &["smb://server/share".into()],
            String::new()
        )
        .is_err()
    );
    assert!(
        Command::from_uris(
            Operation::Folders,
            &["file://remote/tmp".into()],
            String::new()
        )
        .is_err()
    );
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
            for (method, operation) in [
                ("ShowFolders", Operation::Folders),
                ("ShowItems", Operation::Items),
                ("ShowItemProperties", Operation::Properties),
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
                assert_eq!(command.operation, operation);
                assert_eq!(command.startup_id, "caller-token");
                assert_eq!(
                    command.targets.len(),
                    if operation == Operation::Folders {
                        2
                    } else {
                        1
                    }
                );
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
            server._connection.close().await.expect("release provider");
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
            assert_eq!(request.operation, Operation::NewWindow);
            assert_eq!(request.targets[0].directory, PathBuf::from("/tmp"));
            assert!(request.targets[0].selection.is_empty());
            other._connection.close().await.expect("close Files");
            client.close().await.expect("close other provider");
        });
}
