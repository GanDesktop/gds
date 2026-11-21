use std::path::PathBuf;

use gds::service::remote::Address;

use super::{
    Target, classify_uri_payload, pending_removals_include, preview_slice, renamed_remote_address,
    selection_has_dotfile, server_affected_tab_ids,
};
use crate::entities::location::{
    Location,
    places::Places,
    volumes::{DeviceAction, DeviceKind, Drive, Snapshot, Volume, VolumeId},
};
use crate::pages::workspace::model::Tab;

fn sample_volume(attachment: &str, drive: &str) -> Volume {
    Volume {
        id: VolumeId {
            stable: Some(format!("uuid-{attachment}")),
            attachment: attachment.to_owned(),
        },
        object: format!("/org/freedesktop/UDisks2/block_devices/{attachment}"),
        drive: Some(drive.to_owned()),
        name: attachment.to_owned(),
        size: 1024,
        label: attachment.to_owned(),
        filesystem: "ext4".to_owned(),
        read_only: false,
        locked: false,
        mountable: true,
        system_mount: false,
        mount_points: vec![PathBuf::from(format!("/run/media/{attachment}"))],
        visible: true,
    }
}

#[test]
fn item_targets_group_by_parent_and_deduplicate_selections() {
    let targets = Target::items(
        ["/tmp/a", "/", "/tmp/folder", "/tmp/a"]
            .map(PathBuf::from)
            .to_vec(),
    );
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].directory, PathBuf::from("/tmp"));
    assert_eq!(
        targets[0].selection,
        [PathBuf::from("/tmp/a"), PathBuf::from("/tmp/folder")]
    );
    assert_eq!(targets[1].directory, PathBuf::from("/"));
    assert_eq!(targets[1].selection, [PathBuf::from("/")]);
}

#[test]
fn folder_targets_open_each_directory_once_without_selection() {
    let targets = Target::folders(
        ["/tmp/folder", "/", "/tmp/folder"]
            .map(PathBuf::from)
            .to_vec(),
    );
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].directory, PathBuf::from("/tmp/folder"));
    assert!(targets[0].selection.is_empty());
    assert_eq!(targets[1].directory, PathBuf::from("/"));
    assert!(targets[1].selection.is_empty());
}

#[test]
fn classify_uri_payload_separates_local_and_remote_uris() {
    let payload = classify_uri_payload(&[
        "file:///tmp/demo.txt".to_owned(),
        "sftp://alice@example.com/srv/notes.md".to_owned(),
        "/tmp/plain".to_owned(),
    ]);
    assert_eq!(
        payload.local,
        vec![PathBuf::from("/tmp/demo.txt"), PathBuf::from("/tmp/plain")]
    );
    assert_eq!(payload.remote.len(), 1);
    assert_eq!(payload.remote[0].host(), "example.com");
}

#[test]
fn renamed_remote_address_validates_and_replaces_leaf_preserving_host() {
    let addr =
        Address::parse("sftp://alice@example.com/srv/old.txt").expect("valid remote test address");
    let renamed = renamed_remote_address(&addr, "new.txt").expect("valid renamed target");
    assert_eq!(renamed.name(), "new.txt");
    assert_eq!(renamed.host(), "example.com");
    assert!(renamed_remote_address(&addr, "bad/name").is_err());
}

#[test]
fn selection_has_dotfile_detects_hidden_targets_only_on_leaf_filename() {
    assert!(!selection_has_dotfile(&[
        PathBuf::from("/home/tester/.config/visible.toml"),
        PathBuf::from("/tmp/report.txt"),
    ]));
    assert!(selection_has_dotfile(&[
        PathBuf::from("/tmp/report.txt"),
        PathBuf::from("/tmp/.gitignore"),
    ]));
}

#[test]
fn server_affected_tab_ids_matches_all_tabs_sharing_disconnected_server() {
    let places = Places::default();
    let addr_a1 = Address::parse("sftp://alice@alpha.example/srv").expect("valid sftp a1");
    let addr_a2 = Address::parse("sftp://alice@alpha.example/var/log").expect("valid sftp a2");
    let addr_b = Address::parse("sftp://alice@beta.example/srv").expect("valid sftp b");
    let key_alpha = addr_a1.connection_key();

    let tabs = vec![
        Tab::new(10, Location::Remote(addr_a1), &places, false),
        Tab::new(
            20,
            Location::from(PathBuf::from("/tmp").as_path()),
            &places,
            false,
        ),
        Tab::new(30, Location::Remote(addr_b), &places, false),
        Tab::new(40, Location::Remote(addr_a2), &places, true),
    ];

    assert_eq!(server_affected_tab_ids(&tabs, &key_alpha), vec![10, 40]);
}

#[test]
fn pending_removals_marks_all_sibling_drive_volumes_busy_while_draining() {
    let drive_usb = Drive {
        id: "usb-drive-1".to_owned(),
        name: "USB Stick".to_owned(),
        kind: DeviceKind::Usb,
        can_power_off: true,
        can_eject: true,
        sibling_id: Some("usb-enclosure-1".to_owned()),
    };
    let drive_other = Drive {
        id: "nvme-drive-1".to_owned(),
        name: "Internal NVMe".to_owned(),
        kind: DeviceKind::Disk,
        can_power_off: false,
        can_eject: false,
        sibling_id: None,
    };
    let vol_usb_p1 = sample_volume("sdb1", "usb-drive-1");
    let vol_usb_p2 = sample_volume("sdb2", "usb-drive-1");
    let vol_nvme = sample_volume("nvme0n1p2", "nvme-drive-1");

    let snapshot = Snapshot {
        drives: vec![drive_usb, drive_other],
        volumes: vec![vol_usb_p1.clone(), vol_usb_p2.clone(), vol_nvme.clone()],
    };
    let pending = vec![DeviceAction::Unmount(vol_usb_p1.id.clone())];

    assert!(pending_removals_include(
        &snapshot,
        &pending,
        &vol_usb_p1.id
    ));
    assert!(pending_removals_include(
        &snapshot,
        &pending,
        &vol_usb_p2.id
    ));
    assert!(!pending_removals_include(&snapshot, &pending, &vol_nvme.id));
}

#[test]
fn preview_slice_caps_preview_at_five_items_and_reports_overflow() {
    let names = vec![
        "one".to_owned(),
        "two".to_owned(),
        "three".to_owned(),
        "four".to_owned(),
        "five".to_owned(),
        "six".to_owned(),
        "seven".to_owned(),
    ];
    let (items, overflow) = preview_slice(names);
    assert_eq!(items.len(), 5);
    assert_eq!(items[4], "five");
    assert_eq!(overflow, 2);
}
