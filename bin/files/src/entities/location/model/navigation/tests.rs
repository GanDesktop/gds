use super::*;

#[test]
fn tag_history_preserves_uri_and_rejects_stale_results() {
    let tag = Location::from_uri("tags:/工作/设计").expect("parse tag location");
    let mut navigation = Navigation::<()>::new(Location::Recent, None);
    let old = navigation.prepare_open(tag.clone(), None, ReadOptions::default());
    let current = navigation.prepare_open(tag.clone(), None, ReadOptions::default());
    assert_eq!(
        navigation
            .finish_load(&old, Ok(None))
            .expect("finish stale load"),
        LoadOutcome::Stale
    );
    assert_eq!(
        navigation
            .finish_load(&current, Ok(None))
            .expect("finish current load"),
        LoadOutcome::Applied
    );
    assert_eq!(navigation.location(), &tag);
    assert!(navigation.path().is_none());
    assert!(navigation.directory().is_none());
    let back = navigation
        .prepare_history(0, None, ReadOptions::default())
        .expect("prepare back navigation");
    navigation
        .finish_load(&back, Ok(None))
        .expect("finish back navigation");
    assert_eq!(navigation.location(), &Location::Recent);
    let forward = navigation
        .prepare_history(1, None, ReadOptions::default())
        .expect("prepare forward navigation");
    navigation
        .finish_load(&forward, Ok(None))
        .expect("finish forward navigation");
    assert_eq!(navigation.location(), &tag);
}

#[test]
fn base_directory_loads_share_arc_snapshots_and_accept_in_place_updates() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let folder = root.path().join("folder");
    std::fs::create_dir(&folder).expect("create folder");
    std::fs::write(folder.join("item.txt"), "hello").expect("write child item");

    let location = Location::from(root.path());
    let mut navigation = Navigation::<()>::new(location.clone(), Some(root.path().to_path_buf()));
    let request = navigation.prepare_open(
        location,
        Some(root.path().to_path_buf()),
        ReadOptions::default(),
    );
    let cancel = CancellationToken::new();
    let loaded = request.load_base(&cancel).expect("load base snapshot");
    assert_eq!(
        navigation
            .finish_load(&request, Ok(loaded))
            .expect("commit base snapshot"),
        LoadOutcome::Applied
    );

    let arc_snapshot = navigation.directory_arc().expect("shared snapshot arc");
    assert_eq!(
        arc_snapshot
            .entry_for_path(&folder)
            .and_then(|entry| entry.details.children),
        None
    );

    let full_snapshot = request
        .load()
        .expect("load full snapshot")
        .expect("local snapshot present");
    assert!(navigation.update_directory(navigation.revision(), full_snapshot));
    assert_eq!(
        navigation
            .directory()
            .and_then(|snapshot| snapshot.entry_for_path(&folder))
            .and_then(|entry| entry.details.children),
        Some(directory::ChildCount {
            count: 1,
            truncated: false,
        })
    );
}
