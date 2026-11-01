//! The backend talks only to a private bus and temporary picker executables.

use std::{
    io::{BufRead as _, BufReader},
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::{Child, Command},
};

use zbus::zvariant::{OwnedValue, Value};

use super::*;

struct Bus {
    process: Child,
    address: String,
    _config: tempfile::TempDir,
}

impl Bus {
    fn new() -> Self {
        let config = tempfile::tempdir().expect("private bus configuration");
        let config_path = config.path().join("bus.conf");
        std::fs::write(
            &config_path,
            r#"<busconfig>
          <type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
          <policy context="default">
            <allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/>
          </policy>
        </busconfig>"#,
        )
        .expect("bus configuration");
        let mut process = Command::new("dbus-daemon")
            .args(["--nofork", "--print-address=1", "--config-file"])
            .arg(&config_path)
            .stdout(Stdio::piped())
            .spawn()
            .expect("private D-Bus daemon");
        let mut address = String::new();
        BufReader::new(process.stdout.take().expect("daemon stdout"))
            .read_line(&mut address)
            .expect("private bus address");
        assert!(!address.is_empty());
        Self {
            process,
            address: address.trim().into(),
            _config: config,
        }
    }

    async fn connect(&self) -> Connection {
        zbus::connection::Builder::address(self.address.as_str())
            .expect("bus address")
            .build()
            .await
            .expect("bus connection")
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        self.process.kill().expect("stop private bus");
        self.process.wait().expect("reap private bus");
    }
}

fn value(value: impl Into<Value<'static>>) -> OwnedValue {
    value.into().try_to_owned().expect("test variant")
}

fn clone_options(options: &Options) -> Options {
    options
        .iter()
        .map(|(key, value)| (key.clone(), value.try_clone().expect("clone variant")))
        .collect()
}

fn path(suffix: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(format!("{OBJECT_PATH}/request/test/{suffix}")).expect("request path")
}

async fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for request state"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

struct Fixture {
    root: tempfile::TempDir,
    executable: PathBuf,
    file: PathBuf,
    client: Connection,
    server: Connection,
    chooser: FileChooser,
    _bus: Bus,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary files");
        let executable = root.path().join("picker");
        let file = root.path().join("a file.txt");
        std::fs::write(&file, []).expect("selected file");
        let bus = Bus::new();
        let client = bus.connect().await;
        let chooser = FileChooser::new(executable.clone().into_os_string());
        let server = zbus::connection::Builder::address(bus.address.as_str())
            .expect("private address")
            .serve_at(OBJECT_PATH, chooser.clone())
            .expect("chooser interface")
            .name(BUS_NAME)
            .expect("backend name")
            .build()
            .await
            .expect("GDS backend");
        Self {
            root,
            executable,
            file,
            client,
            server,
            chooser,
            _bus: bus,
        }
    }

    fn script(&self, body: &str) {
        let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
        std::fs::write(
            &self.executable,
            format!("#!{}\n{body}\n", Path::new(&shell).display()),
        )
        .expect("picker script");
        std::fs::set_permissions(&self.executable, std::fs::Permissions::from_mode(0o755))
            .expect("executable script");
    }

    async fn call(&self, member: &str, suffix: &str, options: Options) -> Response {
        let reply = self
            .client
            .call_method(
                Some(BUS_NAME),
                OBJECT_PATH,
                Some(CHOOSER),
                member,
                &(path(suffix), "test.app", "wayland:parent", "Pick", options),
            )
            .await
            .expect("FileChooser reply");
        reply.body().deserialize().expect("ua{sv} response")
    }

    async fn close(&self, suffix: &str) {
        self.client
            .call_method(Some(BUS_NAME), path(suffix), Some(REQUEST), "Close", &())
            .await
            .expect("request Close");
    }

    async fn finish(self) {
        self.chooser.shutdown().await;
        self.server.close().await.expect("close GDS connection");
        self.client.close().await.expect("close client connection");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn native_modes_preserve_request_and_return_standard_results() {
    let fixture = Fixture::new().await;
    let choices = vec![(
        "encoding".to_owned(),
        "Encoding".to_owned(),
        vec![("utf8".to_owned(), "UTF-8".to_owned())],
        "utf8".to_owned(),
    )];
    let options = Options::from([
        ("choices".into(), value(choices)),
        ("modal".into(), false.into()),
        ("accept_label".into(), value("_Use")),
        (
            "files".into(),
            value(vec![b"one.txt\0".to_vec(), b"two.txt\0".to_vec()]),
        ),
    ]);
    for member in ["OpenFile", "SaveFile", "SaveFiles"] {
        let paths = if member == "SaveFiles" {
            vec![
                fixture.root.path().join("one.txt").into_os_string(),
                fixture.root.path().join("two.txt").into_os_string(),
            ]
        } else {
            vec![fixture.file.clone().into_os_string()]
        };
        let result = gds::service::desktop::protocol::chooser::Result {
            paths,
            read_only: false,
            filter_index: 0,
            choices: vec![("encoding".into(), "utf8".into())],
        };
        let capture = fixture.root.path().join("request.json");
        let json = serde_json::to_string(&result)
            .expect("native result")
            .replace('\'', "'\\''");
        fixture.script(&format!(
            "cat > '{}'\nprintf '%s\\n' '{json}'",
            capture.display()
        ));
        let (status, result) = fixture
            .call(member, "native", clone_options(&options))
            .await;
        assert_eq!(status, 0);
        let request: gds::service::desktop::protocol::chooser::Request =
            serde_json::from_slice(&std::fs::read(capture).expect("captured stdin"))
                .expect("native request");
        assert_eq!(request.title, "Pick");
        assert_eq!(request.app_id, "test.app");
        assert_eq!(request.parent_window, "wayland:parent");
        assert!(!request.modal);
        assert_eq!(request.accept_label.as_deref(), Some("_Use"));
        assert_eq!(
            Vec::<String>::try_from(result["uris"].try_clone().expect("uris"))
                .expect("list")
                .len(),
            if member == "SaveFiles" { 2 } else { 1 }
        );
        assert!(result.contains_key("choices"));
    }
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_and_invalid_output_do_not_launch_another_backend() {
    let fixture = Fixture::new().await;
    fixture.script("cat > /dev/null\nexit 1");
    assert_eq!(
        fixture.call("OpenFile", "cancelled", Options::new()).await,
        (1, Options::new())
    );
    fixture.script("cat > /dev/null\nprintf 'invalid-json'");
    assert_eq!(
        fixture.call("SaveFile", "invalid", Options::new()).await.0,
        2
    );
    fixture.finish().await;
}

fn alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

fn pids(root: &Path) -> Vec<i32> {
    std::fs::read_dir(root)
        .expect("fixture directory")
        .filter_map(|entry| {
            entry
                .ok()?
                .file_name()
                .to_str()?
                .strip_prefix("pid-")?
                .parse()
                .ok()
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_one_request_reaps_only_its_own_picker() {
    let fixture = Fixture::new().await;
    fixture.script(&format!(
        "cat > /dev/null\nprintf '%s' \"$$\" > '{}/pid-'\"$$\"\nexec sleep 30",
        fixture.root.path().display()
    ));
    let first = fixture.call("OpenFile", "first", Options::new());
    let other = async {
        wait_until(|| pids(fixture.root.path()).len() == 1).await;
        let first_pid = pids(fixture.root.path())[0];
        let second = fixture.call("SaveFile", "second", Options::new());
        let close = async {
            wait_until(|| pids(fixture.root.path()).len() == 2).await;
            fixture.close("first").await;
            wait_until(|| !alive(first_pid)).await;
            let second_pid = pids(fixture.root.path())
                .into_iter()
                .find(|pid| *pid != first_pid)
                .expect("second picker");
            assert!(alive(second_pid));
            fixture.close("second").await;
        };
        tokio::join!(second, close).0
    };
    let (first, second) = tokio::join!(first, other);
    assert_eq!(first, (1, Options::new()));
    assert_eq!(second, (1, Options::new()));
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_cancels_and_reaps_native_save_requests() {
    let fixture = Fixture::new().await;
    fixture.script("cat > /dev/null\nexec sleep 30");
    let pending = fixture.call("SaveFile", "shutdown", Options::new());
    let shutdown = async {
        wait_until(|| {
            !fixture
                .chooser
                .requests
                .lock()
                .expect("requests")
                .active
                .is_empty()
        })
        .await;
        fixture.chooser.shutdown().await;
    };
    let (response, ()) = tokio::join!(pending, shutdown);
    assert_eq!(response, (1, Options::new()));
    assert!(
        fixture
            .chooser
            .requests
            .lock()
            .expect("requests")
            .active
            .is_empty()
    );
    fixture.finish().await;
}
