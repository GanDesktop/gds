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

#[test]
fn load_requests_track_recent_visit_intent_across_initial_refresh_and_navigation() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let pinned_dir = root.path().join("pinned");
    let child_dir = pinned_dir.join("child");
    std::fs::create_dir_all(&child_dir).expect("create pinned and child directories");

    let initial_location = Location::from(pinned_dir.as_path());
    let child_location = Location::from(child_dir.as_path());
    let mut navigation = Navigation::<()>::new(initial_location.clone(), Some(pinned_dir.clone()));

    let restored =
        navigation.prepare_initial(Some(pinned_dir.clone()), ReadOptions::default(), false);
    assert!(!restored.records_recent());

    let refresh_pending_restore =
        navigation.prepare_refresh(Some(pinned_dir.clone()), ReadOptions::default());
    assert!(!refresh_pending_restore.records_recent());

    let restored_snapshot = refresh_pending_restore
        .load()
        .expect("load restored pinned directory");
    assert_eq!(
        navigation
            .finish_load(&refresh_pending_restore, Ok(restored_snapshot))
            .expect("commit restored tab"),
        LoadOutcome::Applied
    );

    let later_refresh =
        navigation.prepare_refresh(Some(pinned_dir.clone()), ReadOptions::default());
    assert!(!later_refresh.records_recent());

    let user_open = navigation.prepare_open(
        child_location,
        Some(child_dir.clone()),
        ReadOptions::default(),
    );
    assert!(user_open.records_recent());
    let child_snapshot = user_open.load().expect("load child directory");
    assert_eq!(
        navigation
            .finish_load(&user_open, Ok(child_snapshot))
            .expect("commit child navigation"),
        LoadOutcome::Applied
    );

    let back = navigation
        .prepare_history(0, Some(pinned_dir.clone()), ReadOptions::default())
        .expect("prepare back navigation");
    assert!(back.records_recent());

    let mut opened_tab = Navigation::<()>::new(initial_location, Some(pinned_dir.clone()));
    let user_tab =
        opened_tab.prepare_initial(Some(pinned_dir.clone()), ReadOptions::default(), true);
    assert!(user_tab.records_recent());
    let refresh_pending_open = opened_tab.prepare_refresh(Some(pinned_dir), ReadOptions::default());
    assert!(refresh_pending_open.records_recent());
}

#[test]
fn progressive_partial_loads_commit_route_once_and_keep_loading_until_finish() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let start_dir = root.path().join("start");
    let slow_dir = root.path().join("slow");
    std::fs::create_dir_all(&start_dir).expect("create start directory");
    std::fs::create_dir_all(&slow_dir).expect("create slow directory");
    for index in 0..6 {
        std::fs::write(slow_dir.join(format!("item-{index:02}.txt")), "x").expect("write item");
    }

    let start_loc = Location::from(start_dir.as_path());
    let slow_loc = Location::from(slow_dir.as_path());
    let mut navigation = Navigation::<()>::new(start_loc.clone(), Some(start_dir.clone()));
    let initial = navigation.prepare_initial(Some(start_dir.clone()), ReadOptions::default(), true);
    let initial_snap = initial.load().expect("load start");
    navigation
        .finish_load(&initial, Ok(initial_snap))
        .expect("commit start");
    assert_eq!(navigation.history().len(), 1);

    let stale = navigation.prepare_open(
        slow_loc.clone(),
        Some(slow_dir.clone()),
        ReadOptions::default(),
    );
    let request = navigation.prepare_open(
        slow_loc.clone(),
        Some(slow_dir.clone()),
        ReadOptions::default(),
    );
    let cancel = CancellationToken::new();
    let mut partials = Vec::new();
    let final_snap = request
        .load_base_streaming(
            directory::StreamOptions {
                initial_delay: std::time::Duration::ZERO,
                interval: std::time::Duration::ZERO,
                min_entries: 2,
            },
            &cancel,
            |partial| partials.push(partial),
        )
        .expect("stream slow directory")
        .expect("local snapshot");
    assert!(partials.len() >= 2);

    // Stale requests are rejected without mutating route or loading state.
    assert_eq!(
        navigation.apply_partial_load(&stale, Arc::clone(&partials[0])),
        PartialLoadOutcome::Stale
    );
    assert_eq!(navigation.location(), &start_loc);
    assert!(!navigation.had_partial_commit(&request));

    // First partial commits the history entry once while keeping `is_loading()` true.
    assert_eq!(
        navigation.apply_partial_load(&request, Arc::clone(&partials[0])),
        PartialLoadOutcome::FirstCommit
    );
    assert!(navigation.is_loading());
    assert!(navigation.had_partial_commit(&request));
    assert_eq!(navigation.location(), &slow_loc);
    assert_eq!(navigation.history().len(), 2);
    assert_eq!(
        navigation.directory().expect("partial snapshot").entries.len(),
        partials[0].entries.len()
    );

    // Subsequent partials update the snapshot in place without pushing duplicate history visits.
    assert_eq!(
        navigation.apply_partial_load(&request, Arc::clone(&partials[1])),
        PartialLoadOutcome::Updated
    );
    assert!(navigation.is_loading());
    assert_eq!(navigation.history().len(), 2);
    assert_eq!(
        navigation.directory().expect("second partial").entries.len(),
        partials[1].entries.len()
    );

    // Final completion clears `is_loading()` and keeps history length at 2.
    assert_eq!(
        navigation
            .finish_load(&request, Ok(Some(final_snap)))
            .expect("finish streamed load"),
        LoadOutcome::Applied
    );
    assert!(!navigation.is_loading());
    assert!(!navigation.had_partial_commit(&request));
    assert_eq!(navigation.history().len(), 2);
    assert_eq!(navigation.directory().expect("final snapshot").entries.len(), 6);
}
