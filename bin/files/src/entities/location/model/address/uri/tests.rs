use super::*;

#[test]
fn built_in_locations_align_with_standard_uris_and_keep_legacy_aliases() {
    assert_standard_root_round_trip(
        "trash:///",
        Location::Trash,
        &[
            "trash:/",
            "TRASH:///",
            "location:/trash",
            "location:/trash/",
        ],
        &[
            "trash://host",
            "trash://",
            "trash:///sub",
            "trash:/sub",
            "trash:///?q=1",
            "trash:///#frag",
            "location://trash",
            "location:///trash",
        ],
    );
    assert_standard_root_round_trip(
        "recent:///",
        Location::Recent,
        &[
            "recent:/",
            "recentlyused:/",
            "recentlyused:///",
            "location:/recent",
            "location:/recent/",
        ],
        &[
            "recent://host",
            "recent://",
            "recent:///sub",
            "recentlyused://host",
            "location://recent",
            "location:///recent",
        ],
    );
    assert_standard_root_round_trip(
        "network:///",
        Location::Network,
        &[
            "network:/",
            "remote:/",
            "remote:///",
            "location:/network",
            "location:/network/",
        ],
        &[
            "network://host",
            "network://",
            "network:///sub",
            "remote://host",
            "location://network",
            "location:///network",
        ],
    );
    assert_builtin_location_round_trip(
        "favorites",
        Location::Favorites,
        &["starred:///", "starred:/"],
    );
    assert_builtin_location_round_trip(
        "volumes",
        Location::Volumes,
        &["computer:///", "computer:/"],
    );
    assert_volume_round_trip(VolumeIdentity::Stable("卷 #/%?".into()));
    assert_volume_round_trip(VolumeIdentity::Attachment("device/42".into()));
    assert!(
        crate::entities::location::scheme::parse("location:/volume/stable/disk?path=../escape")
            .is_err()
    );
    assert!(
        crate::entities::location::scheme::parse("location:/volume/stable/disk?path=%2Fabsolute")
            .is_err()
    );
    assert!(
        crate::entities::location::scheme::parse("location:/volume/stable/disk?path=#fragment")
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn volume_uri_preserves_non_utf8_relative_path() {
    use std::os::unix::ffi::OsStringExt;
    let location = Location::Volume {
        identity: VolumeIdentity::Stable("disk".into()),
        relative: OsString::from_vec(b"folder/\xff".to_vec()),
    };
    let uri = location.uri().expect("format volume URI");
    assert_eq!(
        crate::entities::location::scheme::parse(&uri).expect("parse non-UTF-8 volume URI"),
        location
    );
}

fn assert_standard_root_round_trip(
    canonical: &str,
    location: Location,
    aliases: &[&str],
    rejected: &[&str],
) {
    assert_eq!(location.uri().expect("standard virtual URI"), canonical);
    assert_eq!(
        crate::entities::location::scheme::parse(canonical).expect("parse canonical virtual URI"),
        location
    );
    for alias in aliases {
        assert_eq!(
            crate::entities::location::scheme::parse(alias).expect("parse alias virtual URI"),
            location,
            "alias `{alias}` must resolve to {location:?}"
        );
    }
    for bad in rejected {
        assert!(
            crate::entities::location::scheme::parse(bad).is_err(),
            "malformed URI `{bad}` must be rejected"
        );
    }
}

fn assert_builtin_location_round_trip(name: &str, location: Location, extra_aliases: &[&str]) {
    let uri = format!("location:/{name}");
    assert_eq!(location.uri().expect("built-in location URI"), uri);
    assert_eq!(
        crate::entities::location::scheme::parse(&uri).expect("parse built-in URI"),
        location
    );
    assert_eq!(
        crate::entities::location::scheme::parse(&format!("{uri}/")).expect("parse trailing slash"),
        location
    );
    for alias in extra_aliases {
        assert_eq!(
            crate::entities::location::scheme::parse(alias).expect("parse extra alias"),
            location
        );
    }
    assert!(crate::entities::location::scheme::parse(&format!("location://{name}")).is_err());
    assert!(crate::entities::location::scheme::parse(&format!("location:///{name}")).is_err());
}

fn assert_volume_round_trip(identity: VolumeIdentity) {
    let location = Location::Volume {
        identity,
        relative: "设计/文件 #1".into(),
    };
    let uri = location.uri().expect("volume URI");
    assert!(uri.starts_with("location:/volume/"));
    assert_eq!(
        crate::entities::location::scheme::parse(&uri).expect("parse volume URI"),
        location
    );
    assert!(
        crate::entities::location::scheme::parse(&uri.replacen("location:/", "location://", 1))
            .is_err()
    );
    assert!(
        crate::entities::location::scheme::parse(&uri.replacen("location:/", "location:///", 1))
            .is_err()
    );
}
