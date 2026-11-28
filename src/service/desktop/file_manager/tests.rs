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
        [
            raw.clone(),
            PathBuf::from("/tmp/folder"),
            PathBuf::from("/")
        ]
    );
    assert_eq!(
        local_path_with_host("file://workstation/tmp/a%20%23%FF", Some("Workstation"))
            .expect("RFC 8089 local hostname preserving raw Unix bytes"),
        raw
    );
    assert_eq!(
        local_path_with_host("file://LOCALHOST/tmp/folder", None)
            .expect("case-insensitive localhost"),
        PathBuf::from("/tmp/folder")
    );
    assert!(local_path("smb://server/share").is_err());
    assert!(local_path("file://remote/tmp").is_err());
    assert!(local_path_with_host("file://other-host/tmp", Some("workstation")).is_err());
    assert!(local_path_with_host("file://user@workstation/tmp", Some("workstation")).is_err());
    assert!(local_path_with_host("file://workstation:8080/tmp", Some("workstation")).is_err());
    assert!(local_path_with_host("file:///tmp/a%00b", Some("workstation")).is_err());
}

#[test]
fn show_folders_validates_local_host_network_and_virtual_uris() {
    let accepted = vec![
        "file:///tmp/folder".to_owned(),
        "file://localhost/tmp/folder".to_owned(),
        "file://WORKSTATION/tmp/folder".to_owned(),
        "trash:///".to_owned(),
        "recent:///".to_owned(),
        "starred:///".to_owned(),
        "network:///".to_owned(),
        "computer:///".to_owned(),
        "tags:/work".to_owned(),
        "location:/favorites".to_owned(),
        "smb://files.local/shared".to_owned(),
        "sftp://alice@files.local:2222/srv".to_owned(),
    ];
    assert_eq!(
        folder_uris_with_host(&accepted, Some("workstation")).expect("supported folder URIs"),
        accepted
    );

    for rejected in [
        "",
        "not a uri",
        "/tmp/plain/path",
        "file://other-host/tmp/folder",
        "file://workstation/tmp/folder?q=1",
        "file://workstation/tmp/folder#frag",
        "smb://",
        "sftp://",
        "trash://host/item",
        "https://example.org/folder",
    ] {
        assert!(
            folder_uri_with_host(rejected, Some("workstation")).is_err(),
            "ShowFolders must reject invalid URI `{rejected}`"
        );
    }
}

#[test]
fn application_object_path_converts_valid_names_and_rejects_invalid_inputs() {
    assert_eq!(
        application_object_path(APP_BUS_NAME).expect("standard application bus name"),
        "/io/github/GanDesktop/GdsFiles"
    );
    assert_eq!(
        application_object_path("org.example.my-app").expect("hyphenated application bus name"),
        "/org/example/my_app"
    );
    assert!(matches!(
        application_object_path(""),
        Err(Error::InvalidBusName(_))
    ));
    assert!(matches!(
        application_object_path("single"),
        Err(Error::InvalidBusName(_))
    ));
    assert!(matches!(
        application_object_path(":1.42"),
        Err(Error::InvalidBusName(_))
    ));
    assert!(matches!(
        application_object_path(".leading.dot"),
        Err(Error::InvalidBusName(_))
    ));
    assert!(matches!(
        application_object_path("trailing.dot."),
        Err(Error::InvalidBusName(_))
    ));
    assert!(matches!(
        application_object_path("double..dot"),
        Err(Error::InvalidBusName(_))
    ));
    assert!(matches!(
        application_object_path("org.1digit"),
        Err(Error::InvalidBusName(_))
    ));
    assert!(matches!(
        application_object_path("org.example/bad"),
        Err(Error::InvalidBusName(_))
    ));
    assert!(matches!(
        application_object_path("org.example bad"),
        Err(Error::InvalidBusName(_))
    ));
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
            let file_uris = vec![
                "file:///tmp/a%20name".to_owned(),
                "file:///tmp/another".to_owned(),
            ];
            for (method, request) in [
                (
                    "ShowFolders",
                    Request::ShowFolders {
                        uris: file_uris.clone(),
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
                        &(file_uris.clone(), "caller-token"),
                    )
                    .await
                    .expect("standard call");
                let command = receiver.recv().await.expect("delivered request");
                assert_eq!(command.request, request);
                assert_eq!(command.startup_id, "caller-token");
            }
            let non_file_folders =
                vec!["trash:///".to_owned(), "smb://files.local/share".to_owned()];
            client
                .call_method(
                    Some(BUS_NAME),
                    OBJECT_PATH,
                    Some(BUS_NAME),
                    "ShowFolders",
                    &(non_file_folders.clone(), "folder-token"),
                )
                .await
                .expect("ShowFolders accepts virtual and network URIs");
            let command = receiver.recv().await.expect("delivered ShowFolders");
            assert_eq!(
                command.request,
                Request::ShowFolders {
                    uris: non_file_folders,
                }
            );
            assert_eq!(command.startup_id, "folder-token");
            let invalid_folder = client
                .call_method(
                    Some(BUS_NAME),
                    OBJECT_PATH,
                    Some(BUS_NAME),
                    "ShowFolders",
                    &(vec!["https://example.org/file"], ""),
                )
                .await;
            assert!(invalid_folder.is_err());
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

            let app_path = application_object_path(APP_BUS_NAME).expect("application object path");
            let mut activate_data: HashMap<&str, Value<'_>> = HashMap::new();
            activate_data.insert("activation-token", Value::from("xdg-activation-token"));
            activate_data.insert("desktop-startup-id", Value::from("fallback-startup-id"));
            client
                .call_method(
                    Some(APP_BUS_NAME),
                    app_path.as_str(),
                    Some(APPLICATION_INTERFACE),
                    "Activate",
                    &activate_data,
                )
                .await
                .expect("Application.Activate with activation-token");
            let command = receiver.recv().await.expect("delivered Activate");
            assert_eq!(command.request, Request::Activate { new_window: false });
            assert_eq!(command.startup_id, "xdg-activation-token");

            let fallback_data =
                HashMap::from([("desktop-startup-id", Value::from("legacy-startup-id"))]);
            client
                .call_method(
                    Some(APP_BUS_NAME),
                    app_path.as_str(),
                    Some(APPLICATION_INTERFACE),
                    "Activate",
                    &fallback_data,
                )
                .await
                .expect("Application.Activate with desktop-startup-id fallback");
            let command = receiver.recv().await.expect("delivered Activate fallback");
            assert_eq!(command.request, Request::Activate { new_window: false });
            assert_eq!(command.startup_id, "legacy-startup-id");

            let empty_data: HashMap<&str, Value<'_>> = HashMap::new();
            client
                .call_method(
                    Some(APP_BUS_NAME),
                    app_path.as_str(),
                    Some(APPLICATION_INTERFACE),
                    "Activate",
                    &empty_data,
                )
                .await
                .expect("Application.Activate without platform_data");
            let command = receiver.recv().await.expect("delivered empty Activate");
            assert_eq!(command.request, Request::Activate { new_window: false });
            assert_eq!(command.startup_id, "");

            let open_uris = vec![
                "file:///tmp/a%20name".to_owned(),
                "trash:///".to_owned(),
                "smb://files.local/share".to_owned(),
            ];
            let open_data = HashMap::from([("activation-token", Value::from("open-token"))]);
            client
                .call_method(
                    Some(APP_BUS_NAME),
                    app_path.as_str(),
                    Some(APPLICATION_INTERFACE),
                    "Open",
                    &(open_uris.clone(), open_data),
                )
                .await
                .expect("Application.Open with local, virtual, and network URIs");
            let command = receiver.recv().await.expect("delivered Open");
            assert_eq!(
                command.request,
                Request::OpenLocations {
                    uris: open_uris,
                    new_window: false,
                }
            );
            assert_eq!(command.startup_id, "open-token");

            let invalid_open = client
                .call_method(
                    Some(APP_BUS_NAME),
                    app_path.as_str(),
                    Some(APPLICATION_INTERFACE),
                    "Open",
                    &(vec!["https://example.org/file"], empty_data.clone()),
                )
                .await;
            assert!(invalid_open.is_err());

            let empty_open_data =
                HashMap::from([("desktop-startup-id", Value::from("empty-open-id"))]);
            client
                .call_method(
                    Some(APP_BUS_NAME),
                    app_path.as_str(),
                    Some(APPLICATION_INTERFACE),
                    "Open",
                    &(Vec::<String>::new(), empty_open_data),
                )
                .await
                .expect("Application.Open with empty URI list");
            let command = receiver.recv().await.expect("delivered empty Open");
            assert_eq!(command.request, Request::Activate { new_window: false });
            assert_eq!(command.startup_id, "empty-open-id");

            let action_data =
                HashMap::from([("activation-token", Value::from("new-window-action-token"))]);
            client
                .call_method(
                    Some(APP_BUS_NAME),
                    app_path.as_str(),
                    Some(APPLICATION_INTERFACE),
                    "ActivateAction",
                    &("new-window", Vec::<Value<'_>>::new(), action_data),
                )
                .await
                .expect("Application.ActivateAction(new-window)");
            let command = receiver.recv().await.expect("delivered ActivateAction");
            assert_eq!(command.request, Request::Activate { new_window: true });
            assert_eq!(command.startup_id, "new-window-action-token");

            let unknown_action = client
                .call_method(
                    Some(APP_BUS_NAME),
                    app_path.as_str(),
                    Some(APPLICATION_INTERFACE),
                    "ActivateAction",
                    &("unknown", Vec::<Value<'_>>::new(), empty_data),
                )
                .await
                .expect_err("unknown ActivateAction must return InvalidArgs");
            assert!(
                matches!(
                    unknown_action,
                    zbus::Error::MethodError(ref name, _, _)
                        if name.as_str() == "org.freedesktop.DBus.Error.InvalidArgs"
                ),
                "expected InvalidArgs, got {unknown_action}"
            );

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
            let uris = vec!["trash:///".to_owned(), "file:///tmp".to_owned()];
            Server::open_existing_locations(APP_BUS_NAME, &uris, false, "multi-token")
                .await
                .expect("dispatch multiple URIs to existing window");
            let request = receiver.recv().await.expect("multi-location request");
            assert_eq!(request.startup_id, "multi-token");
            assert_eq!(
                request.request,
                Request::OpenLocations {
                    uris: uris.clone(),
                    new_window: false,
                }
            );
            Server::open_existing_locations(APP_BUS_NAME, &uris, true, "new-win-token")
                .await
                .expect("dispatch multiple URIs to new window");
            let request = receiver.recv().await.expect("new-window location request");
            assert_eq!(request.startup_id, "new-win-token");
            assert_eq!(
                request.request,
                Request::OpenLocations {
                    uris,
                    new_window: true,
                }
            );
            assert!(
                Server::open_existing_locations(APP_BUS_NAME, &[], false, "")
                    .await
                    .is_err()
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
    assert_eq!(
        <Application as zbus::object_server::Interface>::name().as_str(),
        APPLICATION_INTERFACE
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

#[test]
fn show_items_short_circuits_empty_and_absolutizes_paths() {
    use std::path::Path;

    let empty: &[PathBuf] = &[];
    tokio::runtime::Runtime::new()
        .expect("Tokio")
        .block_on(async {
            show_items(empty, "")
                .await
                .expect("empty show_items should short-circuit without session bus");
        });

    let expected_abs = std::path::absolute(Path::new("nested/../capture.png")).expect("abs");
    let expected_uri = url::Url::from_file_path(&expected_abs)
        .expect("file url")
        .to_string();

    let rel_slices: &[&Path] = &[Path::new("nested/../capture.png")];
    assert_eq!(
        encode_show_item_uris(rel_slices).expect("encode relative path"),
        vec![expected_uri]
    );

    let owned_slices = vec![PathBuf::from("/tmp/example image.png")];
    assert_eq!(
        encode_show_item_uris(&owned_slices).expect("encode owned path"),
        vec!["file:///tmp/example%20image.png".to_owned()]
    );
}
