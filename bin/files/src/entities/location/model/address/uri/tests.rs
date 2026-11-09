use super::*;

#[test]
fn built_in_locations_use_paths_without_authorities() {
    assert_builtin_round_trip("recent", Location::Recent);
    assert_builtin_round_trip("favorites", Location::Favorites);
    assert_builtin_round_trip("network", Location::Network);
    assert_builtin_round_trip("trash", Location::Trash);
    assert_builtin_round_trip("volumes", Location::Volumes);
    assert_volume_round_trip(VolumeIdentity::Stable("卷 #/%?".into()));
    assert_volume_round_trip(VolumeIdentity::Attachment("device/42".into()));
    assert!(Location::from_uri("location:/volume/stable/disk?path=../escape").is_err());
    assert!(Location::from_uri("location:/volume/stable/disk?path=%2Fabsolute").is_err());
    assert!(Location::from_uri("location:/volume/stable/disk?path=#fragment").is_err());
}

#[cfg(unix)]
#[test]
fn volume_uri_preserves_non_utf8_relative_path() {
    use std::os::unix::ffi::OsStringExt;
    let location = Location::Volume {
        identity: VolumeIdentity::Stable("disk".into()),
        relative: OsString::from_vec(b"folder/\xff".to_vec()),
    };
    assert_eq!(
        Location::from_uri(&location.uri().unwrap()).unwrap(),
        location
    );
}

fn assert_builtin_round_trip(name: &str, location: Location) {
    let uri = format!("location:/{name}");
    assert_eq!(location.uri().expect("built-in location URI"), uri);
    assert_eq!(
        Location::from_uri(&uri).expect("parse built-in URI"),
        location
    );
    assert_eq!(
        Location::from_uri(&format!("{uri}/")).expect("parse trailing slash"),
        location
    );
    assert!(Location::from_uri(&format!("location://{name}")).is_err());
    assert!(Location::from_uri(&format!("location:///{name}")).is_err());
}

fn assert_volume_round_trip(identity: VolumeIdentity) {
    let location = Location::Volume {
        identity,
        relative: "设计/文件 #1".into(),
    };
    let uri = location.uri().expect("volume URI");
    assert!(uri.starts_with("location:/volume/"));
    assert_eq!(
        Location::from_uri(&uri).expect("parse volume URI"),
        location
    );
    assert!(Location::from_uri(&uri.replacen("location:/", "location://", 1)).is_err());
    assert!(Location::from_uri(&uri.replacen("location:/", "location:///", 1)).is_err());
}
