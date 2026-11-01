//! Exercise the actual wire signatures against an isolated D-Bus service.
//! These tests never connect to the system bus or operate a real disk.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};

use super::*;

const DRIVE_PATH: &str = "/org/freedesktop/UDisks2/drives/Test";
const VOLUME_PATHS: [&str; 2] = [
    "/org/freedesktop/UDisks2/block_devices/test1",
    "/org/freedesktop/UDisks2/block_devices/test2",
];

#[derive(Default)]
struct State {
    labels: [Option<String>; 2],
    label_error: bool,
    mounted: [bool; 2],
    busy_second: bool,
    optical: bool,
    system_root: bool,
    uuid_suffix: String,
    calls: Vec<String>,
}

type Shared = Arc<Mutex<State>>;

fn string(value: &str) -> OwnedValue {
    Value::from(value).try_to_owned().expect("string value")
}
fn path(value: &str) -> OwnedValue {
    Value::from(OwnedObjectPath::try_from(value).expect("object path"))
        .try_to_owned()
        .expect("object path value")
}

struct Manager(Shared);
#[zbus::interface(name = "org.freedesktop.DBus.ObjectManager")]
impl Manager {
    fn get_managed_objects(&self) -> Objects {
        let state = self.0.lock().expect("fixture state");
        let drive = Properties::from([
            ("Model".into(), string("Test disk")),
            ("Serial".into(), string("serial")),
            ("ConnectionBus".into(), string("usb")),
            ("CanPowerOff".into(), true.into()),
            ("Ejectable".into(), true.into()),
            ("Optical".into(), state.optical.into()),
            ("MediaAvailable".into(), true.into()),
            ("TimeDetected".into(), 10_u64.into()),
        ]);
        let mut objects = Objects::from([(
            OwnedObjectPath::try_from(DRIVE_PATH).expect("drive path"),
            Interfaces::from([(DRIVE.into(), drive)]),
        )]);
        for (index, object) in VOLUME_PATHS.iter().enumerate() {
            let block = Properties::from([
                ("Drive".into(), path(DRIVE_PATH)),
                (
                    "IdLabel".into(),
                    string(
                        &state.labels[index]
                            .clone()
                            .unwrap_or_else(|| format!("Data {}", index + 1)),
                    ),
                ),
                (
                    "IdUUID".into(),
                    string(&format!("uuid-{index}{}", state.uuid_suffix)),
                ),
                ("IdType".into(), string("ext4")),
                ("Size".into(), 1_000_000_u64.into()),
                ("DeviceNumber".into(), (u64::MAX - index as u64).into()),
            ]);
            let points = if state.mounted[index] {
                vec![if index == 0 && state.system_root {
                    b"/\0".to_vec()
                } else {
                    format!("/media/test{}\0", index + 1).into_bytes()
                }]
            } else {
                Vec::<Vec<u8>>::new()
            };
            let filesystem = Properties::from([(
                "MountPoints".into(),
                Value::from(points).try_to_owned().expect("mount points"),
            )]);
            objects.insert(
                OwnedObjectPath::try_from(*object).expect("volume path"),
                Interfaces::from([(BLOCK.into(), block), (FILESYSTEM.into(), filesystem)]),
            );
        }
        objects
    }
}

struct Filesystem {
    state: Shared,
    index: usize,
}
#[zbus::interface(name = "org.freedesktop.UDisks2.Filesystem")]
impl Filesystem {
    fn set_label(
        &self,
        label: &str,
        _options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<()> {
        let mut state = self.state.lock().expect("fixture state");
        if state.label_error {
            return Err(zbus::fdo::Error::Failed(
                "filesystem refuses this label".into(),
            ));
        }
        state.labels[self.index] = Some(label.to_owned());
        state
            .calls
            .push(format!("set-label:{}:{label}", self.index));
        Ok(())
    }

    fn mount(&self, _options: HashMap<String, OwnedValue>) -> String {
        let mut state = self.state.lock().expect("fixture state");
        state.mounted[self.index] = true;
        state.calls.push(format!("mount:{}", self.index));
        format!("/media/test{}", self.index + 1)
    }

    fn unmount(&self, options: HashMap<String, OwnedValue>) -> zbus::fdo::Result<()> {
        assert!(!options.contains_key("force"), "never force unmount");
        let mut state = self.state.lock().expect("fixture state");
        state.calls.push(format!("unmount:{}", self.index));
        if self.index == 1 && state.busy_second {
            return Err(zbus::fdo::Error::Failed("device busy".into()));
        }
        state.mounted[self.index] = false;
        Ok(())
    }
}

struct Disk(Shared);
#[zbus::interface(name = "org.freedesktop.UDisks2.Drive")]
impl Disk {
    fn power_off(&self, _options: HashMap<String, OwnedValue>) {
        self.0
            .lock()
            .expect("fixture state")
            .calls
            .push("power-off".into());
    }
    fn eject(&self, _options: HashMap<String, OwnedValue>) {
        self.0
            .lock()
            .expect("fixture state")
            .calls
            .push("eject".into());
    }
}

struct Bus {
    process: Child,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let mut config = tempfile::NamedTempFile::new().expect("private bus configuration");
        config
            .write_all(include_bytes!("tests/assets/dbus-session.conf"))
            .expect("write private bus configuration");
        let mut process = Command::new("dbus-daemon")
            .arg("--config-file")
            .arg(config.path())
            .args(["--nofork", "--print-address=1", "--nopidfile"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is required for the storage protocol tests");
        let mut address = String::new();
        let length = BufReader::new(process.stdout.take().expect("bus stdout"))
            .read_line(&mut address)
            .expect("private bus address");
        assert!(
            length > 0,
            "dbus-daemon exited without a private bus address"
        );
        Self {
            process,
            address: address.trim().to_owned(),
        }
    }

    async fn serve(&self, state: Shared) -> Connection {
        let mut builder = zbus::connection::Builder::address(self.address.as_str())
            .expect("private address")
            .name(DESTINATION)
            .expect("service name")
            .serve_at(ROOT, Manager(state.clone()))
            .expect("manager")
            .serve_at(DRIVE_PATH, Disk(state.clone()))
            .expect("drive");
        for (index, path) in VOLUME_PATHS.iter().enumerate() {
            builder = builder
                .serve_at(
                    *path,
                    Filesystem {
                        state: state.clone(),
                        index,
                    },
                )
                .expect("filesystem");
        }
        builder.build().await.expect("mock service")
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        self.process.kill().expect("stop private bus");
        self.process.wait().expect("reap private bus");
    }
}

#[tokio::test]
async fn discovers_external_mounts_and_mounts_with_typed_udisks_calls() {
    let bus = Bus::new();
    let state = Shared::default();
    state.lock().expect("fixture").mounted[0] = true;
    let connection = bus.serve(state.clone()).await;
    let snapshot = read(&connection).await.expect("enumerate");
    assert_eq!(snapshot.volumes.len(), 2);
    assert_eq!(
        snapshot.volumes[0].mount_points,
        [PathBuf::from("/media/test1")]
    );
    let id = snapshot.volumes[1].id.clone();
    assert_eq!(
        operate_on(
            &connection,
            snapshot.clone(),
            DeviceAction::Mount(id.clone())
        )
        .await
        .expect("mount"),
        Some("/media/test2".into())
    );
    assert_eq!(state.lock().expect("fixture").calls, ["mount:1"]);
    state.lock().expect("fixture").uuid_suffix = "-different-medium".into();
    assert!(matches!(
        operate_on(&connection, snapshot, DeviceAction::Unmount(id)).await,
        Err(Error::Disconnected)
    ));
    assert_eq!(state.lock().expect("fixture").calls, ["mount:1"]);
}

#[test]
fn shows_mounted_system_volumes_and_keeps_them_browsable() {
    let state = Shared::default();
    state.lock().expect("fixture").mounted = [true, true];
    let mut objects = Manager(state).get_managed_objects();
    let root = objects
        .get_mut(&OwnedObjectPath::try_from(VOLUME_PATHS[0]).expect("root path"))
        .expect("root object");
    root.get_mut(FILESYSTEM).expect("filesystem").insert(
        "MountPoints".into(),
        Value::from(vec![b"/\0".to_vec()])
            .try_to_owned()
            .expect("mount point"),
    );
    root.get_mut(BLOCK)
        .expect("block")
        .insert("HintSystem".into(), true.into());
    let data = objects
        .get_mut(&OwnedObjectPath::try_from(VOLUME_PATHS[1]).expect("data path"))
        .expect("data object");
    data.get_mut(BLOCK)
        .expect("block")
        .insert("HintSystem".into(), true.into());
    let drive = objects
        .get_mut(&OwnedObjectPath::try_from(DRIVE_PATH).expect("drive path"))
        .expect("drive")
        .get_mut(DRIVE)
        .expect("properties");
    drive.insert("ConnectionBus".into(), string("ata"));
    let snapshot = parse(&objects);
    assert!(snapshot.volumes[0].visible);
    assert!(snapshot.volumes[0].system_mount);
    assert!(!snapshot.volumes[0].can_unmount());
    assert!(snapshot.volumes[1].visible);
}

#[tokio::test]
async fn system_root_opens_without_mounting_and_has_no_removal_operations() {
    let bus = Bus::new();
    let state = Shared::default();
    {
        let mut state = state.lock().expect("fixture");
        state.mounted = [true, true];
        state.system_root = true;
    }
    let connection = bus.serve(state.clone()).await;
    let snapshot = read(&connection).await.expect("enumerate");
    let root = snapshot.volumes[0].id.clone();
    assert_eq!(
        operate_on(
            &connection,
            snapshot.clone(),
            DeviceAction::Mount(root.clone())
        )
        .await
        .expect("open mounted root"),
        Some("/".into())
    );
    assert!(matches!(
        operate_on(&connection, snapshot.clone(), DeviceAction::Unmount(root)).await,
        Err(Error::Unsupported)
    ));
    assert!(matches!(
        operate_on(
            &connection,
            snapshot.clone(),
            DeviceAction::PowerOff(snapshot.volumes[1].id.clone())
        )
        .await,
        Err(Error::Unsupported)
    ));
    assert!(state.lock().expect("fixture").calls.is_empty());
}

#[test]
fn optical_media_without_a_filesystem_can_be_ejected_and_empty_drives_stay_hidden() {
    let state = Shared::default();
    state.lock().expect("fixture").optical = true;
    let mut objects = Manager(state).get_managed_objects();
    for path in VOLUME_PATHS {
        objects
            .get_mut(&OwnedObjectPath::try_from(path).expect("volume path"))
            .expect("volume")
            .remove(FILESYSTEM);
    }
    let media = parse(&objects);
    assert!(
        media
            .volumes
            .iter()
            .all(|volume| volume.visible && !volume.mountable)
    );
    let action = DeviceAction::Eject(media.volumes[0].id.clone());
    assert_eq!(media.removal_volumes(&action).len(), 2);
    let drive = objects
        .get_mut(&OwnedObjectPath::try_from(DRIVE_PATH).expect("drive path"))
        .expect("drive")
        .get_mut(DRIVE)
        .expect("properties");
    drive.insert("MediaAvailable".into(), false.into());
    assert!(parse(&objects).volumes.is_empty());
}

#[tokio::test]
async fn changes_filesystem_labels_without_unmounting_and_preserves_errors_and_identity_checks() {
    let bus = Bus::new();
    let state = Shared::default();
    {
        let mut state = state.lock().expect("fixture");
        state.mounted = [true, true];
        state.system_root = true;
    }
    let connection = bus.serve(state.clone()).await;
    let snapshot = read(&connection).await.expect("enumerate");
    let id = snapshot.volumes[0].id.clone();
    let action = DeviceAction::SetLabel {
        volume: id.clone(),
        label: "资料盘".into(),
    };
    assert!(!action.removes());
    operate_on(&connection, snapshot.clone(), action)
        .await
        .expect("set label");
    let after = read(&connection).await.expect("refresh label");
    assert_eq!(after.volumes[0].label, "资料盘");
    assert_eq!(after.volumes[0].name, "资料盘");
    assert_eq!(after.volumes[0].id, id);
    assert_eq!(after.volumes[0].mount_points, [PathBuf::from("/")]);
    assert!(!after.volumes[1].mount_points.is_empty());
    assert_eq!(state.lock().expect("fixture").calls, ["set-label:0:资料盘"]);

    operate_on(
        &connection,
        after,
        DeviceAction::SetLabel {
            volume: id.clone(),
            label: String::new(),
        },
    )
    .await
    .expect("clear label");
    assert_eq!(
        read(&connection)
            .await
            .expect("refresh cleared label")
            .volumes[0]
            .label,
        ""
    );
    state.lock().expect("fixture").label_error = true;
    let result = operate_on(
        &connection,
        snapshot.clone(),
        DeviceAction::SetLabel {
            volume: id.clone(),
            label: "refused".into(),
        },
    )
    .await;
    assert!(
        matches!(result, Err(Error::Operation(reason)) if reason.contains("filesystem refuses this label"))
    );
    assert_eq!(
        read(&connection)
            .await
            .expect("label after failure")
            .volumes[0]
            .label,
        ""
    );

    state.lock().expect("fixture").uuid_suffix = "-replacement".into();
    assert!(matches!(
        operate_on(
            &connection,
            snapshot,
            DeviceAction::SetLabel {
                volume: id,
                label: "wrong medium".into()
            }
        )
        .await,
        Err(Error::Disconnected)
    ));
    assert_eq!(
        state.lock().expect("fixture").calls,
        ["set-label:0:资料盘", "set-label:0:"]
    );
}

#[tokio::test]
async fn unmounting_one_volume_preserves_its_sibling_and_never_powers_off_the_device() {
    let bus = Bus::new();
    let state = Shared::default();
    state.lock().expect("fixture").mounted = [true, true];
    let connection = bus.serve(state.clone()).await;
    let snapshot = read(&connection).await.expect("enumerate");
    let id = snapshot.volumes[0].id.clone();
    operate_on(&connection, snapshot, DeviceAction::Unmount(id))
        .await
        .expect("unmount one volume");
    let after = read(&connection).await.expect("refresh");
    assert!(after.volumes[0].mount_points.is_empty());
    assert!(!after.volumes[1].mount_points.is_empty());
    assert_eq!(state.lock().expect("fixture").calls, ["unmount:0"]);
}

#[tokio::test]
async fn partial_removal_preserves_mount_state_and_never_powers_off_a_busy_drive() {
    let bus = Bus::new();
    let state = Shared::default();
    {
        let mut state = state.lock().expect("fixture");
        state.mounted = [true, true];
        state.busy_second = true;
    }
    let connection = bus.serve(state.clone()).await;
    let snapshot = read(&connection).await.expect("enumerate");
    let id = snapshot.volumes[0].id.clone();
    let result = operate_on(&connection, snapshot, DeviceAction::PowerOff(id.clone())).await;
    assert!(matches!(result, Err(Error::PartialRemoval { .. })));
    let after = read(&connection).await.expect("refresh partial result");
    assert!(after.volumes[0].mount_points.is_empty());
    assert!(!after.volumes[1].mount_points.is_empty());
    assert_eq!(
        state.lock().expect("fixture").calls,
        ["unmount:0", "unmount:1"]
    );
    state.lock().expect("fixture").busy_second = false;
    operate_on(&connection, after, DeviceAction::PowerOff(id.clone()))
        .await
        .expect("retry removal");
    assert_eq!(
        state
            .lock()
            .expect("fixture")
            .calls
            .last()
            .expect("last call"),
        "power-off"
    );
    {
        let mut state = state.lock().expect("fixture");
        state.optical = true;
        state.mounted = [true, false];
        state.calls.clear();
    }
    let optical = read(&connection).await.expect("optical medium");
    operate_on(&connection, optical, DeviceAction::Eject(id))
        .await
        .expect("eject");
    assert_eq!(state.lock().expect("fixture").calls, ["unmount:0", "eject"]);
}
