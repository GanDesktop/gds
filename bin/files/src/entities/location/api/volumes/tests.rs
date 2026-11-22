use std::{
    fs,
    path::{Path, PathBuf},
};

use super::network::{
    FreebsdMountKind, classify_freebsd_mount, discover_network_mounts, is_gvfs_network_entry,
    mounts_from, read_network_mounts, unescape,
};

#[test]
fn unescape_decodes_mountinfo_octal_sequences() {
    let raw = br"/mnt/Share\040With\011Tab\012Line\134Slash";
    assert_eq!(
        unescape(raw),
        b"/mnt/Share With\tTab\nLine\\Slash".as_slice()
    );
}

#[test]
fn mounts_from_filters_network_kinds_and_unescapes_paths() {
    let sample = br#"29 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw
44 29 0:39 / /mnt/nas\040media rw,relatime - nfs4 10.0.0.2:/media rw
45 29 0:40 / /mnt/windows rw,relatime - cifs //server/share rw
46 29 0:41 / relative/path rw,relatime - smb3 //server/rel rw
47 29 0:42 / /run/user/1000/gvfs rw,nosuid,nodev - fuse.gvfsd-fuse gvfsd-fuse rw
"#;
    let parsed = mounts_from(sample);
    assert_eq!(
        parsed,
        vec![
            (PathBuf::from("/mnt/nas media"), b"nfs4".to_vec()),
            (PathBuf::from("/mnt/windows"), b"cifs".to_vec()),
            (
                PathBuf::from("/run/user/1000/gvfs"),
                b"fuse.gvfsd-fuse".to_vec()
            ),
        ]
    );
}

#[test]
fn gvfs_entry_filter_recognizes_supported_network_schemes() {
    assert!(is_gvfs_network_entry(
        b"smb-share:server=nas.local,share=docs"
    ));
    assert!(is_gvfs_network_entry(b"sftp:host=example.com,user=alice"));
    assert!(!is_gvfs_network_entry(b"mtp:host=phone"));
    assert!(!is_gvfs_network_entry(b"gphoto2:host=camera"));
}

#[test]
fn discover_network_mounts_expands_gvfs_and_deduplicates() {
    let temp = tempfile::tempdir().expect("create temporary gvfs directory");
    let sftp_dir = temp.path().join("sftp:host=box.local");
    let smb_dir = temp.path().join("smb-share:server=box.local,share=pub");
    let ignored_dir = temp.path().join("mtp:host=android");
    fs::create_dir(&sftp_dir).expect("create sftp mount entry");
    fs::create_dir(&smb_dir).expect("create smb mount entry");
    fs::create_dir(&ignored_dir).expect("create non-network gvfs entry");

    let gvfs_path = temp.path().to_string_lossy().replace(' ', r"\040");
    let mountinfo = format!(
        "51 29 0:44 / /mnt/nfs rw - nfs4 10.0.0.1:/export rw\n\
         52 29 0:44 / /mnt/nfs rw - nfs4 10.0.0.1:/export rw\n\
         53 29 0:45 / {gvfs_path} rw - fuse.gvfsd-fuse gvfsd-fuse rw\n"
    );

    let discovered =
        discover_network_mounts(mountinfo.as_bytes()).expect("discover network mounts");
    let mut expected = vec![PathBuf::from("/mnt/nfs"), sftp_dir, smb_dir];
    expected.sort();
    assert_eq!(discovered, expected);
}

#[test]
fn freebsd_mount_classifier_handles_network_types_and_mfsnamelen_gvfs() {
    assert_eq!(
        classify_freebsd_mount(b"nfs", Path::new("/mnt/nas")),
        Some(FreebsdMountKind::Direct)
    );
    assert_eq!(
        classify_freebsd_mount(b"smbfs", Path::new("/mnt/smb")),
        Some(FreebsdMountKind::Direct)
    );
    assert_eq!(
        classify_freebsd_mount(b"fusefs.sshfs", Path::new("/mnt/ssh")),
        Some(FreebsdMountKind::Direct)
    );
    assert_eq!(
        classify_freebsd_mount(b"fusefs.gvfsd-fu", Path::new("/var/run/user/1001/gvfs")),
        Some(FreebsdMountKind::Gvfs)
    );
    assert_eq!(
        classify_freebsd_mount(b"fusefs", Path::new("/var/run/user/1001/gvfs")),
        Some(FreebsdMountKind::Gvfs)
    );
    assert_eq!(
        classify_freebsd_mount(b"fusefs", Path::new("/mnt/local-fuse")),
        None
    );
    assert_eq!(classify_freebsd_mount(b"ufs", Path::new("/")), None);
    assert_eq!(classify_freebsd_mount(b"nfs", Path::new("relative")), None);
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[test]
fn live_network_mount_discovery_succeeds() {
    let _ = read_network_mounts().expect("read live kernel network mounts");
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[tokio::test]
async fn udisks_parse_handles_bsdisks_drive_and_volume_properties() {
    use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Str, Value};

    use super::{
        DeviceKind,
        udisks::{Interfaces, Objects, Properties, parse},
    };

    fn path(raw: &str) -> OwnedObjectPath {
        ObjectPath::try_from(raw).expect("valid object path").into()
    }

    fn path_val(raw: &str) -> OwnedValue {
        Value::from(ObjectPath::try_from(raw).expect("valid object path"))
            .try_to_owned()
            .expect("owned object path value")
    }

    fn str_val(raw: &str) -> OwnedValue {
        OwnedValue::from(Str::from(raw))
    }

    let mut objects = Objects::new();

    // Internal disk reported by FreeBSD `bsdisks`: `Removable: true` alongside
    // `bsdisks_IsHotpluggable: false` and `bsdisks_ConnectionBus: "scsi"`.
    let mut sys_drive = Properties::new();
    sys_drive.insert("Vendor".into(), str_val("VirtIO"));
    sys_drive.insert("Model".into(), str_val("Block Device"));
    sys_drive.insert("ConnectionBus".into(), str_val(""));
    sys_drive.insert("bsdisks_ConnectionBus".into(), str_val("scsi"));
    sys_drive.insert("Removable".into(), OwnedValue::from(true));
    sys_drive.insert("MediaRemovable".into(), OwnedValue::from(true));
    sys_drive.insert("bsdisks_IsHotpluggable".into(), OwnedValue::from(false));
    objects.insert(
        path("/org/freedesktop/UDisks2/drives/vtbd0"),
        Interfaces::from([("org.freedesktop.UDisks2.Drive".into(), sys_drive)]),
    );

    // USB flash drive reported by FreeBSD `bsdisks`: `bsdisks_ConnectionBus: "usb"`.
    let mut usb_drive = Properties::new();
    usb_drive.insert("Vendor".into(), str_val("SanDisk"));
    usb_drive.insert("Model".into(), str_val("Ultra"));
    usb_drive.insert("ConnectionBus".into(), str_val(""));
    usb_drive.insert("bsdisks_ConnectionBus".into(), str_val("usb"));
    usb_drive.insert("bsdisks_IsHotpluggable".into(), OwnedValue::from(true));
    usb_drive.insert("CanPowerOff".into(), OwnedValue::from(true));
    objects.insert(
        path("/org/freedesktop/UDisks2/drives/da0"),
        Interfaces::from([("org.freedesktop.UDisks2.Drive".into(), usb_drive)]),
    );

    // Root UFS partition mounted at `/`.
    let mut root_block = Properties::new();
    root_block.insert(
        "Drive".into(),
        path_val("/org/freedesktop/UDisks2/drives/vtbd0"),
    );
    root_block.insert("IdLabel".into(), str_val("rootfs"));
    root_block.insert("IdType".into(), str_val("ufs"));
    root_block.insert("Size".into(), OwnedValue::from(82_000_000_000u64));
    root_block.insert("HintSystem".into(), OwnedValue::from(true));
    root_block.insert("HintIgnore".into(), OwnedValue::from(false));
    let mut root_fs = Properties::new();
    root_fs.insert(
        "MountPoints".into(),
        Value::from(vec![b"/\0".to_vec()])
            .try_to_owned()
            .expect("mount points value"),
    );
    objects.insert(
        path("/org/freedesktop/UDisks2/block_devices/vtbd0p5"),
        Interfaces::from([
            ("org.freedesktop.UDisks2.Block".into(), root_block),
            ("org.freedesktop.UDisks2.Filesystem".into(), root_fs),
        ]),
    );

    // Unmounted internal system partition (`HintSystem: true`, `bsdisks_IsHotpluggable: false`)
    // must NOT be treated as removable just because `bsdisks` sets `Removable: true`.
    let mut unmounted_sys_block = Properties::new();
    unmounted_sys_block.insert(
        "Drive".into(),
        path_val("/org/freedesktop/UDisks2/drives/vtbd0"),
    );
    unmounted_sys_block.insert("IdLabel".into(), str_val("recovery"));
    unmounted_sys_block.insert("IdType".into(), str_val("ufs"));
    unmounted_sys_block.insert("HintSystem".into(), OwnedValue::from(true));
    objects.insert(
        path("/org/freedesktop/UDisks2/block_devices/vtbd0p4"),
        Interfaces::from([
            ("org.freedesktop.UDisks2.Block".into(), unmounted_sys_block),
            (
                "org.freedesktop.UDisks2.Filesystem".into(),
                Properties::new(),
            ),
        ]),
    );

    // Unmounted USB partition (`bsdisks_ConnectionBus: "usb"`, `HintSystem: true` or false)
    // remains visible as a hotpluggable USB volume.
    let mut usb_block = Properties::new();
    usb_block.insert(
        "Drive".into(),
        path_val("/org/freedesktop/UDisks2/drives/da0"),
    );
    usb_block.insert("IdLabel".into(), str_val("DATA"));
    usb_block.insert("IdType".into(), str_val("msdosfs"));
    usb_block.insert("HintSystem".into(), OwnedValue::from(true));
    objects.insert(
        path("/org/freedesktop/UDisks2/block_devices/da0p1"),
        Interfaces::from([
            ("org.freedesktop.UDisks2.Block".into(), usb_block),
            (
                "org.freedesktop.UDisks2.Filesystem".into(),
                Properties::new(),
            ),
        ]),
    );

    let snapshot = parse(&objects).await;
    assert_eq!(
        snapshot
            .drive("/org/freedesktop/UDisks2/drives/vtbd0")
            .map(|d| d.kind),
        Some(DeviceKind::Disk)
    );
    assert_eq!(
        snapshot
            .drive("/org/freedesktop/UDisks2/drives/da0")
            .map(|d| d.kind),
        Some(DeviceKind::Usb)
    );

    let root = snapshot
        .volumes
        .iter()
        .find(|v| v.object.ends_with("/vtbd0p5"))
        .expect("root volume");
    assert!(root.visible);
    assert!(root.system_mount);
    assert!(!root.can_unmount());

    let recovery = snapshot
        .volumes
        .iter()
        .find(|v| v.object.ends_with("/vtbd0p4"))
        .expect("recovery volume");
    assert!(
        !recovery.visible,
        "unmounted internal bsdisks system partition must stay hidden"
    );

    let usb = snapshot
        .volumes
        .iter()
        .find(|v| v.object.ends_with("/da0p1"))
        .expect("usb volume");
    assert!(usb.visible);
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[tokio::test]
async fn live_udisks_refresh_and_capacity_probe_when_bus_available() {
    let Ok(snapshot) = super::udisks::refresh().await else {
        // Sandboxed CI environments without a system D-Bus or UDisks2 daemon skip live probing.
        return;
    };
    let requests =
        super::usage::UsageCache::default().requests(&snapshot, std::time::Instant::now());
    for request in requests {
        let capacity = request
            .read()
            .expect("read mounted volume capacity via fstatvfs");
        assert!(capacity.total > 0);
    }
}
