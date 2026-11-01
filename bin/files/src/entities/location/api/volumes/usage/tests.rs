use super::*;
use crate::entities::location::volumes::Volume;

fn snapshot(root: &str) -> Snapshot {
    Snapshot {
        volumes: vec![Volume {
            id: VolumeId {
                stable: Some("uuid".into()),
                attachment: "attachment".into(),
            },
            object: "backend-object".into(),
            drive: None,
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
        }],
        ..Snapshot::default()
    }
}

#[test]
fn capacity_uses_available_blocks_and_handles_empty_or_extreme_statistics() {
    let capacity = Capacity::from_blocks(4096, 1000, 150).expect("block counts");
    assert_eq!(
        capacity,
        Capacity {
            total: 4_096_000,
            available: 614_400
        }
    );
    assert!((capacity.unavailable_fraction() - 0.85).abs() < f32::EPSILON);
    assert_eq!(
        Capacity::from_blocks(4096, 0, 0)
            .expect("empty filesystem")
            .unavailable_fraction(),
        0.
    );
    assert!(matches!(
        Capacity::from_blocks(4096, u64::MAX, 0),
        Err(ReadError::Overflow)
    ));
    assert!(matches!(
        Capacity::from_blocks(0, 1000, 150),
        Err(ReadError::InvalidBlockSize)
    ));
}

#[test]
fn unmounting_or_remounting_discards_inflight_results_and_reused_paths_do_not_inherit_usage() {
    let mut cache = UsageCache::default();
    let now = Instant::now();
    let original = snapshot("/media/data");
    let id = original.volumes[0].id.clone();
    let old = cache.requests(&original, now).pop().expect("first read");
    assert!(
        cache.requests(&original, now + REFRESH_INTERVAL).is_empty(),
        "one read per volume even when slow"
    );
    cache.requests(&Snapshot::default(), now);
    assert!(cache.get(&id).is_none());
    let current = cache
        .requests(&original, now)
        .pop()
        .expect("remounted volume");
    let capacity = Capacity {
        total: 100,
        available: 30,
    };
    assert!(!cache.finish(old, Ok(capacity), now));
    assert_eq!(cache.get(&id), Some(&CapacityState::Loading));
    assert!(cache.finish(current, Ok(capacity), now));
    let mut replacement = original;
    replacement.volumes[0].id.stable = Some("different-uuid".into());
    cache.requests(&replacement, now);
    assert!(cache.get(&id).is_none());
    assert_eq!(
        cache.get(&replacement.volumes[0].id),
        Some(&CapacityState::Loading)
    );
}

#[test]
fn cached_values_refresh_without_flashing_and_errors_never_become_zero_free_space() {
    let mut cache = UsageCache::default();
    let now = Instant::now();
    let snapshot = snapshot("/media/data");
    let id = &snapshot.volumes[0].id;
    let first = cache.requests(&snapshot, now).pop().expect("initial read");
    let capacity = Capacity {
        total: 100,
        available: 30,
    };
    cache.finish(first, Ok(capacity), now);
    assert!(cache.requests(&snapshot, now).is_empty());
    let refresh = cache
        .requests(&snapshot, now + REFRESH_INTERVAL)
        .pop()
        .expect("refresh due");
    assert_eq!(cache.get(id), Some(&CapacityState::Ready(capacity)));
    cache.finish(
        refresh,
        Err(ReadError::Io(std::io::Error::from(
            std::io::ErrorKind::NotFound,
        ))),
        now + REFRESH_INTERVAL,
    );
    assert!(matches!(cache.get(id), Some(CapacityState::Unavailable(_))));
    cache.invalidate();
    assert_eq!(
        cache.requests(&snapshot, now + REFRESH_INTERVAL).len(),
        1,
        "explicit refresh retries immediately"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reads_the_mounted_filesystems_capacity_without_scanning_its_files() {
    let directory = tempfile::tempdir().expect("fixture");
    let snapshot = snapshot(directory.path().to_str().expect("temporary path"));
    let mut cache = UsageCache::default();
    let request = cache
        .requests(&snapshot, Instant::now())
        .pop()
        .expect("mounted volume read");
    let capacity = request.read().expect("filesystem statistics");
    assert!(capacity.total > 0);
    assert!(capacity.available <= capacity.total);
}
