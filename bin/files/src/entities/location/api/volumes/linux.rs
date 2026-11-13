//! UDisks2's ObjectManager is the source of truth; no shell commands or
//! application-owned mount table. See https://storaged.org/doc/udisks2-api/.

use std::{
    collections::HashMap,
    ffi::OsString,
    os::unix::{ffi::OsStringExt, fs::MetadataExt},
    path::PathBuf,
    time::Duration,
};

use futures_lite::{StreamExt, future};
use tokio::sync::mpsc::Sender;
use zbus::{
    Connection, MatchRule, MessageStream, Proxy,
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

use super::service::Observation;
use super::*;

const DESTINATION: &str = "org.freedesktop.UDisks2";
const ROOT: &str = "/org/freedesktop/UDisks2";
const BLOCK: &str = "org.freedesktop.UDisks2.Block";
const DRIVE: &str = "org.freedesktop.UDisks2.Drive";
const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
const ENCRYPTED: &str = "org.freedesktop.UDisks2.Encrypted";
const PARTITION: &str = "org.freedesktop.UDisks2.Partition";
const RETRY_DELAY: Duration = Duration::from_secs(3);
const RESYNC_INTERVAL: Duration = Duration::from_secs(10);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(120);

type Properties = HashMap<String, OwnedValue>;
type Interfaces = HashMap<String, Properties>;
type Objects = HashMap<OwnedObjectPath, Interfaces>;

fn text(properties: &Properties, key: &str) -> String {
    properties
        .get(key)
        .and_then(|value| <&str>::try_from(value).ok())
        .unwrap_or_default()
        .to_owned()
}

fn flag(properties: &Properties, key: &str) -> bool {
    properties
        .get(key)
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or_default()
}

fn number(properties: &Properties, key: &str) -> u64 {
    properties
        .get(key)
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or_default()
}

fn object(properties: &Properties, key: &str) -> Option<String> {
    let path = properties
        .get(key)
        .and_then(|value| <&zbus::zvariant::ObjectPath>::try_from(value).ok())?;
    (path.as_str() != "/").then(|| path.to_string())
}

fn mounts(properties: &Properties) -> Vec<PathBuf> {
    properties
        .get("MountPoints")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| Vec::<Vec<u8>>::try_from(value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|mut bytes| {
            if bytes.last() == Some(&0) {
                bytes.pop();
            }
            let path = PathBuf::from(OsString::from_vec(bytes));
            path.is_absolute().then_some(path)
        })
        .collect()
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

async fn parse(objects: &Objects) -> Snapshot {
    let mut snapshot = Snapshot::default();
    for (path, interfaces) in objects {
        let Some(drive) = interfaces.get(DRIVE) else {
            continue;
        };
        let optical = flag(drive, "Optical") || text(drive, "Media").starts_with("optical");
        snapshot.drives.push(Drive {
            id: path.to_string(),
            name: [text(drive, "Vendor"), text(drive, "Model")]
                .join(" ")
                .trim()
                .to_owned(),
            kind: if optical {
                DeviceKind::Optical
            } else if text(drive, "ConnectionBus") == "usb" {
                DeviceKind::Usb
            } else {
                DeviceKind::Disk
            },
            can_power_off: flag(drive, "CanPowerOff"),
            can_eject: flag(drive, "Ejectable"),
            sibling_id: nonempty(text(drive, "SiblingId")),
        });
    }
    for (path, interfaces) in objects {
        let Some(block) = interfaces.get(BLOCK) else {
            continue;
        };
        let filesystem = interfaces.get(FILESYSTEM);
        let locked = interfaces.contains_key(ENCRYPTED)
            && !objects.values().any(|candidate| {
                candidate
                    .get(BLOCK)
                    .and_then(|block| object(block, "CryptoBackingDevice"))
                    .as_deref()
                    == Some(path.as_str())
            });
        let drive_id = object(block, "Drive");
        let drive_properties = drive_id
            .as_deref()
            .and_then(|id| objects.iter().find(|(path, _)| path.as_str() == id))
            .and_then(|(_, interfaces)| interfaces.get(DRIVE));
        let drive = drive_id.as_deref().and_then(|id| snapshot.drive(id));
        let optical_media = drive.is_some_and(|drive| drive.kind == DeviceKind::Optical)
            && drive_properties.is_some_and(|drive| flag(drive, "MediaAvailable"));
        if filesystem.is_none() && !locked && !optical_media {
            continue;
        }
        let mount_points = filesystem.map(mounts).unwrap_or_default();
        let removable = drive_properties.is_some_and(|drive| {
            flag(drive, "Removable")
                || flag(drive, "MediaRemovable")
                || text(drive, "ConnectionBus") == "usb"
        });
        let empty_optical = drive.is_some_and(|drive| drive.kind == DeviceKind::Optical)
            && drive_properties.is_some_and(|drive| !flag(drive, "MediaAvailable"));
        let system_location = mount_points.iter().any(|path| {
            matches!(
                path.to_str(),
                Some("/" | "/boot" | "/boot/efi" | "/nix" | "/home" | "/usr" | "/var" | "/efi")
            )
        });
        let already_mounted = !mount_points.is_empty();
        let size = number(block, "Size");
        let name = [
            text(block, "HintName"),
            text(block, "IdLabel"),
            drive.map(|drive| drive.name.clone()).unwrap_or_default(),
        ]
        .into_iter()
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| format!("{:.1} GB", size as f64 / 1_000_000_000.));
        let uuid = nonempty(text(block, "IdUUID"));
        let partition_uuid = interfaces
            .get(PARTITION)
            .and_then(|partition| nonempty(text(partition, "UUID")));
        let physical = drive_properties.and_then(|drive| {
            [text(drive, "WWN"), text(drive, "Serial"), text(drive, "Id")]
                .into_iter()
                .find(|id| !id.is_empty())
        });
        // UUID alone is useful only if unique in the snapshot; reconnect() also
        // checks that. Physical identity distinguishes cloned media when present.
        let stable = uuid
            .or(partition_uuid)
            .map(|volume| format!("{}:{volume}", physical.unwrap_or_default()));
        let detected = drive_properties
            .map(|drive| number(drive, "TimeDetected"))
            .unwrap_or_default();
        let media = drive_properties
            .map(|drive| number(drive, "TimeMediaDetected"))
            .unwrap_or_default();
        let device_number = number(block, "DeviceNumber");
        let sysfs = format!(
            "/sys/dev/block/{}:{}",
            rustix::fs::major(device_number),
            rustix::fs::minor(device_number)
        );
        // kernfs gives a newly attached block object a new inode even if Linux
        // reuses its major/minor pair. This also covers devices without a Drive.
        let kernel_identity = tokio::fs::metadata(sysfs)
            .await
            .map(|metadata| metadata.ino())
            .unwrap_or_default();
        let attachment = format!("{detected}:{media}:{device_number}:{kernel_identity}");
        snapshot.volumes.push(Volume {
            id: VolumeId { stable, attachment },
            object: path.to_string(),
            drive: drive_id,
            name,
            size,
            label: text(block, "IdLabel"),
            filesystem: text(block, "IdType"),
            read_only: flag(block, "ReadOnly"),
            locked,
            mountable: filesystem.is_some(),
            system_mount: system_location,
            mount_points,
            visible: !flag(block, "HintIgnore")
                && !empty_optical
                && (removable || !flag(block, "HintSystem") || already_mounted),
        });
    }
    snapshot.drives.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.volumes.sort_by(|a, b| a.object.cmp(&b.object));
    snapshot
}

/// # Errors
/// Reports unavailable D-Bus/UDisks2 or invalid ObjectManager replies.
async fn read(connection: &Connection) -> Result<Snapshot, Error> {
    let proxy = Proxy::new(
        connection,
        DESTINATION,
        ROOT,
        "org.freedesktop.DBus.ObjectManager",
    )
    .await
    .map_err(unavailable)?;
    let objects: Objects = proxy
        .call("GetManagedObjects", &())
        .await
        .map_err(unavailable)?;
    Ok(parse(&objects).await)
}

fn unavailable(error: zbus::Error) -> Error {
    Error::Unavailable(error.to_string())
}

/// # Errors
/// Returns a service error when the system bus or UDisks2 cannot be queried.
pub(super) async fn refresh() -> Result<Snapshot, Error> {
    let connection = Connection::system().await.map_err(unavailable)?;
    read(&connection).await
}

pub(super) async fn watch(sender: Sender<Observation>) {
    while !sender.is_closed() {
        if let Err(error) = watch_connection(&sender).await
            && sender
                .send(Observation {
                    started: std::time::Instant::now(),
                    result: Err(error),
                })
                .await
                .is_err()
        {
            return;
        }
        tokio::time::sleep(RETRY_DELAY).await;
    }
}

/// # Errors
/// Exits on a lost bus connection; the caller reports it and retries.
async fn watch_connection(sender: &Sender<Observation>) -> Result<(), Error> {
    let connection = Connection::system().await.map_err(unavailable)?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path_namespace(ROOT)
        .map_err(unavailable)?
        .build();
    // Subscribe before enumeration to avoid losing an insertion between them.
    let mut signals = MessageStream::for_match_rule(rule, &connection, Some(256))
        .await
        .map_err(unavailable)?;
    loop {
        let started = std::time::Instant::now();
        let result = read(&connection).await;
        match result {
            Ok(snapshot) => {
                if sender
                    .send(Observation {
                        started,
                        result: Ok(snapshot),
                    })
                    .await
                    .is_err()
                {
                    return Ok(());
                }
            }
            Err(error) => return Err(error),
        }
        // Periodic resync also detects service restarts, even if its last event
        // was NameOwnerChanged outside UDisks2's object namespace.
        let signal = future::or(async { Some(signals.next().await) }, async {
            tokio::time::sleep(RESYNC_INTERVAL).await;
            None
        })
        .await;
        match signal {
            None | Some(Some(Ok(_))) => {}
            Some(Some(Err(error))) => return Err(unavailable(error)),
            Some(None) => return Err(Error::Unavailable("system bus disconnected".into())),
        }
    }
}

/// # Errors
/// Rejects changed identity or unsupported actions. Partial unmounts remain in
/// system state; the service always refreshes after this call, including errors.
pub(super) async fn operate(
    expected: Snapshot,
    action: DeviceAction,
) -> Result<Option<PathBuf>, Error> {
    let connection = zbus::connection::Builder::system()
        .map_err(unavailable)?
        .method_timeout(OPERATION_TIMEOUT)
        .build()
        .await
        .map_err(unavailable)?;
    operate_on(&connection, expected, action).await
}

/// # Errors
/// Returns the first incomplete step; earlier successful unmounts are retained.
async fn operate_on(
    connection: &Connection,
    expected: Snapshot,
    action: DeviceAction,
) -> Result<Option<PathBuf>, Error> {
    let current = read(connection).await?;
    let volume = current.volume(action.volume()).ok_or(Error::Disconnected)?;
    let old = expected
        .volume(action.volume())
        .ok_or(Error::Disconnected)?;
    if volume.object != old.object {
        return Err(Error::Disconnected);
    }
    if let DeviceAction::SetLabel { label, .. } = &action {
        if !volume.can_set_label() {
            return Err(Error::Unsupported);
        }
        // https://storaged.org/doc/udisks2-api/latest/gdbus-org.freedesktop.UDisks2.Filesystem.html#gdbus-method-org-freedesktop-UDisks2-Filesystem.SetLabel
        let proxy = Proxy::new(connection, DESTINATION, volume.object.as_str(), FILESYSTEM)
            .await
            .map_err(unavailable)?;
        let result: Result<(), _> = proxy
            .call("SetLabel", &(label.as_str(), HashMap::<&str, Value>::new()))
            .await;
        result.map_err(operation_error)?;
        return Ok(None);
    }
    if let DeviceAction::Mount(_) = action {
        if let Some(path) = volume.mount_points.first() {
            return Ok(Some(path.clone()));
        }
        if volume.locked {
            return Err(Error::Locked);
        }
        if !volume.mountable {
            return Err(Error::Unsupported);
        }
        let proxy = Proxy::new(&connection, DESTINATION, volume.object.as_str(), FILESYSTEM)
            .await
            .map_err(unavailable)?;
        let path: String = proxy
            .call("Mount", &(HashMap::<&str, Value>::new(),))
            .await
            .map_err(operation_error)?;
        return Ok(Some(PathBuf::from(path)));
    }
    if !current.supports_removal(&action) {
        return Err(Error::Unsupported);
    }
    let drive = volume.drive.as_deref().and_then(|id| current.drive(id));
    let members = current.removal_volumes(&action);
    if members
        .iter()
        .any(|member| expected.volume(&member.id).is_none())
    {
        return Err(Error::Disconnected);
    }
    let mut unmounted = Vec::new();
    for member in members {
        if member.mount_points.is_empty() {
            continue;
        }
        let latest = read(connection).await?;
        if !latest
            .volume(&member.id)
            .is_some_and(|volume| volume.object == member.object)
        {
            return Err(removal_error(&unmounted, Error::Disconnected.to_string()));
        }
        let proxy = Proxy::new(&connection, DESTINATION, member.object.as_str(), FILESYSTEM)
            .await
            .map_err(unavailable)?;
        let result: Result<(), _> = proxy
            .call("Unmount", &(HashMap::<&str, Value>::new(),))
            .await;
        if let Err(error) = result {
            return Err(removal_error(
                &unmounted,
                format!("{}: {error}", member.name),
            ));
        }
        unmounted.push(member.name.clone());
    }
    let method = match action {
        DeviceAction::PowerOff(_) => Some("PowerOff"),
        DeviceAction::Eject(_) => Some("Eject"),
        _ => None,
    };
    if let Some(method) = method {
        let latest = read(connection).await?;
        if latest.volume(action.volume()).is_none()
            || latest
                .removal_volumes(&action)
                .iter()
                .any(|volume| expected.volume(&volume.id).is_none())
        {
            return Err(removal_error(&unmounted, Error::Disconnected.to_string()));
        }
        let drive = drive.ok_or(Error::Unsupported)?;
        let proxy = Proxy::new(&connection, DESTINATION, drive.id.as_str(), DRIVE)
            .await
            .map_err(unavailable)?;
        let result: Result<(), _> = proxy.call(method, &(HashMap::<&str, Value>::new(),)).await;
        result.map_err(|error| removal_error(&unmounted, error.to_string()))?;
    }
    Ok(None)
}

fn operation_error(error: zbus::Error) -> Error {
    Error::Operation(error.to_string())
}

fn removal_error(unmounted: &[String], error: String) -> Error {
    if unmounted.is_empty() {
        Error::Operation(error)
    } else {
        Error::PartialRemoval {
            unmounted: unmounted.join(", "),
            reason: error,
        }
    }
}
