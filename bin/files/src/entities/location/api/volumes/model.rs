use std::path::{Path, PathBuf};

/// Stable identity, when the medium supplies one, plus the observed attachment.
/// The attachment prevents a newly inserted disk reusing a device node from
/// inheriting a tab. Neither field is a mount path.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VolumeId {
    pub stable: Option<String>,
    pub attachment: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceKind {
    Disk,
    Usb,
    Optical,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drive {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub can_power_off: bool,
    pub can_eject: bool,
    /// Drives sharing a USB enclosure must be treated as one removal scope.
    pub sibling_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub id: VolumeId,
    /// Backend routing only; never used as a persistent identity or row label.
    pub object: String,
    pub drive: Option<String>,
    pub name: String,
    pub size: u64,
    pub label: String,
    pub filesystem: String,
    pub read_only: bool,
    pub locked: bool,
    pub mountable: bool,
    /// Mounted as an active system location; it remains browsable.
    pub system_mount: bool,
    pub mount_points: Vec<PathBuf>,
    pub visible: bool,
}

impl Volume {
    pub fn can_set_label(&self) -> bool {
        self.mountable && !self.read_only && !self.locked
    }

    pub fn can_unmount(&self) -> bool {
        !self.system_mount && !self.mount_points.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub drives: Vec<Drive>,
    /// Includes hidden system volumes, so removing a drive cannot omit one.
    pub volumes: Vec<Volume>,
}

impl Snapshot {
    pub fn volume(&self, id: &VolumeId) -> Option<&Volume> {
        self.volumes.iter().find(|volume| &volume.id == id)
    }

    pub fn drive(&self, id: &str) -> Option<&Drive> {
        self.drives.iter().find(|drive| drive.id == id)
    }

    pub fn at_path(&self, path: &Path) -> Option<&Volume> {
        self.volumes
            .iter()
            .filter(|volume| {
                volume.visible
                    && volume
                        .mount_points
                        .iter()
                        .any(|root| path.starts_with(root))
            })
            .max_by_key(|volume| {
                volume
                    .mount_points
                    .iter()
                    .filter(|root| path.starts_with(root))
                    .map(|root| root.components().count())
                    .max()
                    .unwrap_or_default()
            })
    }

    /// A reconnect needs a unique stable identity; duplicated UUIDs are not proof.
    pub fn reconnect(&self, id: &VolumeId) -> Option<&Volume> {
        if let Some(volume) = self.volume(id) {
            return Some(volume);
        }
        let stable = id.stable.as_ref()?;
        let mut matches = self
            .volumes
            .iter()
            .filter(|volume| volume.id.stable.as_ref() == Some(stable));
        let volume = matches.next()?;
        matches.next().is_none().then_some(volume)
    }

    pub fn removal_volumes(&self, action: &DeviceAction) -> Vec<&Volume> {
        let Some(volume) = self.volume(action.volume()) else {
            return Vec::new();
        };
        if matches!(
            action,
            DeviceAction::Mount(_) | DeviceAction::Unmount(_) | DeviceAction::SetLabel { .. }
        ) {
            return vec![volume];
        }
        let Some(drive) = volume.drive.as_deref().and_then(|id| self.drive(id)) else {
            return Vec::new();
        };
        self.volumes
            .iter()
            .filter(|volume| {
                volume
                    .drive
                    .as_deref()
                    .and_then(|id| self.drive(id))
                    .is_some_and(|candidate| {
                        candidate.id == drive.id
                            || (matches!(action, DeviceAction::PowerOff(_))
                                && drive.sibling_id.is_some()
                                && candidate.sibling_id == drive.sibling_id)
                    })
            })
            .collect()
    }
}

impl Snapshot {
    pub fn supports_removal(&self, action: &DeviceAction) -> bool {
        let members = self.removal_volumes(action);
        if members.is_empty() || members.iter().any(|volume| volume.system_mount) {
            return false;
        }
        let volume = members[0];
        let drive = volume.drive.as_deref().and_then(|id| self.drive(id));
        match action {
            DeviceAction::Unmount(_) => volume.can_unmount(),
            DeviceAction::PowerOff(_) => {
                drive.is_some_and(|drive| drive.kind == DeviceKind::Usb && drive.can_power_off)
            }
            DeviceAction::Eject(_) => drive.is_some_and(|drive| drive.can_eject),
            DeviceAction::Mount(_) | DeviceAction::SetLabel { .. } => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceAction {
    Mount(VolumeId),
    SetLabel { volume: VolumeId, label: String },
    Unmount(VolumeId),
    PowerOff(VolumeId),
    Eject(VolumeId),
}

impl DeviceAction {
    pub fn volume(&self) -> &VolumeId {
        match self {
            Self::Mount(id) | Self::Unmount(id) | Self::PowerOff(id) | Self::Eject(id) => id,
            Self::SetLabel { volume, .. } => volume,
        }
    }

    pub fn removes(&self) -> bool {
        matches!(self, Self::Unmount(_) | Self::PowerOff(_) | Self::Eject(_))
    }
}

#[derive(Debug)]
pub struct OperationOutcome {
    pub mount_point: Option<PathBuf>,
    pub snapshot: Snapshot,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "UDisks2 is unavailable. Enable services.udisks2 and a polkit authentication agent: {0}"
    )]
    Unavailable(String),
    #[error("this device is no longer connected, or its identity changed")]
    Disconnected,
    #[error("this device does not support the requested operation")]
    Unsupported,
    #[error("this encrypted volume must be unlocked before it can be mounted")]
    Locked,
    #[error("a device operation is already in progress")]
    Busy,
    #[error("{0}")]
    Operation(String),
    #[error("Already unmounted: {unmounted}. Removal is incomplete: {reason}")]
    PartialRemoval { unmounted: String, reason: String },
}
