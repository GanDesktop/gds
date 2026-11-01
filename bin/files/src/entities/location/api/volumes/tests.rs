use super::*;

fn volume(uuid: Option<&str>, attachment: &str, drive: &str, root: &str) -> Volume {
    Volume {
        id: VolumeId {
            stable: uuid.map(str::to_owned),
            attachment: attachment.into(),
        },
        object: attachment.into(),
        drive: Some(drive.into()),
        name: "Data".into(),
        size: 0,
        label: "Data".into(),
        filesystem: "ext4".into(),
        read_only: false,
        locked: false,
        mountable: true,
        system_mount: false,
        mount_points: vec![root.into()],
        visible: true,
    }
}

#[test]
fn reconnect_requires_identity_and_refuses_duplicate_uuids() {
    let old = volume(Some("uuid"), "old", "usb", "/media/data");
    let mut snapshot = Snapshot {
        volumes: vec![volume(Some("other"), "new", "usb", "/media/data")],
        ..Snapshot::default()
    };
    assert!(snapshot.reconnect(&old.id).is_none());
    snapshot.volumes = vec![volume(Some("uuid"), "new", "usb", "/media/new")];
    assert_eq!(
        snapshot
            .reconnect(&old.id)
            .expect("unique medium")
            .mount_points[0],
        std::path::Path::new("/media/new")
    );
    snapshot
        .volumes
        .push(volume(Some("uuid"), "clone", "usb", "/media/clone"));
    assert!(snapshot.reconnect(&old.id).is_none());
    let anonymous = volume(None, "old", "usb", "/media/data");
    assert!(snapshot.reconnect(&anonymous.id).is_none());
}

#[test]
fn whole_drive_removal_includes_hidden_partitions_and_usb_siblings() {
    let mut hidden = volume(Some("system"), "2", "drive-a", "/boot");
    hidden.visible = false;
    let first = volume(Some("data"), "1", "drive-a", "/media/data");
    let snapshot = Snapshot {
        drives: ["drive-a", "drive-b"]
            .map(|id| Drive {
                id: id.into(),
                name: id.into(),
                kind: DeviceKind::Usb,
                can_power_off: true,
                can_eject: false,
                sibling_id: Some("enclosure".into()),
            })
            .into(),
        volumes: vec![
            first.clone(),
            hidden,
            volume(Some("second"), "3", "drive-b", "/media/second"),
        ],
    };
    assert_eq!(
        snapshot
            .removal_volumes(&DeviceAction::Unmount(first.id.clone()))
            .len(),
        1
    );
    assert_eq!(
        snapshot
            .removal_volumes(&DeviceAction::PowerOff(first.id.clone()))
            .len(),
        3
    );
    assert_eq!(
        snapshot
            .removal_volumes(&DeviceAction::Eject(first.id))
            .len(),
        2
    );
}
