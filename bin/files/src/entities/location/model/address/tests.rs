use super::*;

#[test]
fn every_location_has_a_round_trippable_uri() {
    let local = tempfile::tempdir().expect("local root");
    let mut locations = vec![
        Location::from(&local.path().join("中文 #100%.txt")),
        Location::from_uri("sftp://alice@host:2222/team").expect("remote"),
        Location::Recent,
        Location::Favorites,
        Location::Network,
        Location::Trash,
        Location::Volumes,
        Location::Volume {
            identity: VolumeIdentity::Stable("disk / #1".into()),
            relative: "photos/2026".into(),
        },
        Location::Volume {
            identity: VolumeIdentity::Attachment("/org/device/1".into()),
            relative: "".into(),
        },
    ];
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        locations.push(Location::Directory(OsString::from_vec(
            b"/tmp/caf\xe9".to_vec(),
        )));
        locations.push(Location::Volume {
            identity: VolumeIdentity::Stable("disk".into()),
            relative: OsString::from_vec(b"caf\xe9".to_vec()),
        });
    }
    for location in locations {
        let uri = location.uri().expect("location URI");
        assert_eq!(
            Location::from_uri(&uri).expect("parse URI"),
            location,
            "{uri}"
        );
    }
    assert!(Location::from_uri("location://missing").is_err());
    assert!(Location::from_uri("location://volume/stable/disk?path=%2Fetc").is_err());
    assert!(Location::from_uri("location://volume/stable/disk?path=..%2Fetc").is_err());
}

#[cfg(unix)]
#[test]
fn local_uri_preserves_parent_symlink_traversal() {
    use std::{fs, os::unix::fs::symlink};
    let root = tempfile::tempdir().expect("URI fixture");
    fs::create_dir_all(root.path().join("actual/nested")).expect("target");
    symlink(root.path().join("actual/nested"), root.path().join("alias")).expect("alias");
    let location = Location::from(&root.path().join("alias/../leaf"));
    let uri = location.uri().expect("URI");
    assert_eq!(
        Location::from_uri(&uri).expect("parsed URI"),
        Location::from(&root.path().join("actual/leaf"))
    );
}
