use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_windows_share_a_pool_and_preserve_independent_edits() {
    let root = tempfile::tempdir().expect("fixture");
    let storage = Arc::new(Storage::new(root.path().join("state.sqlite")));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);

    let first_storage = storage.clone();
    let first_barrier = barrier.clone();
    let first_scope = scope.clone();
    let first_load = tokio::spawn(async move {
        first_barrier.wait().await;
        first_storage
            .load(&first_scope)
            .await
            .expect("first window state")
    });

    let second_storage = storage.clone();
    let second_barrier = barrier.clone();
    let second_scope = scope.clone();
    let second_load = tokio::spawn(async move {
        second_barrier.wait().await;
        second_storage
            .load(&second_scope)
            .await
            .expect("second window state")
    });

    let (first, second) = tokio::join!(first_load, second_load);
    let mut first = first.expect("first window task");
    let mut second = second.expect("second window task");
    let store = first.store.clone().expect("shared store");
    assert!(Arc::ptr_eq(
        first.store.as_ref().expect("first store"),
        &store
    ));
    assert!(Arc::ptr_eq(
        second.store.as_ref().expect("second store"),
        &store
    ));
    assert!(!first.has_pending());
    assert!(!second.has_pending());

    first.set_favorite(Path::new("/first").into(), true);
    assert!(!second.has_pending());
    assert!(!second.is_favorite(Path::new("/first")));

    first.set_favorite(Path::new("/window-0").into(), true);
    second.set_favorite(Path::new("/window-1").into(), true);
    let first_save = first.take_save().expect("first pending edit");
    let second_save = second.take_save().expect("second pending edit");
    let (first_saved, second_saved) = tokio::join!(
        tokio::spawn(async move { first_save.run().await.expect("save first window") }),
        tokio::spawn(async move { second_save.run().await.expect("save second window") }),
    );
    first_saved.expect("first save task");
    second_saved.expect("second save task");

    let reopened = storage.load(&scope).await.expect("fresh window state");
    assert!(Arc::ptr_eq(
        reopened.store.as_ref().expect("reopened store"),
        &store
    ));
    assert!(reopened.is_favorite(Path::new("/first")));
    assert!(reopened.is_favorite(Path::new("/window-0")));
    assert!(reopened.is_favorite(Path::new("/window-1")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_another_window_does_not_initialize_storage_again() {
    use gds::service::storage::{Database, TransactionMode};
    use toasty_driver_sqlite::Sqlite;

    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().join("state.sqlite");
    let storage = Storage::new(path.clone());
    // The application must retain the pool even after its window state is dropped.
    drop(
        storage
            .load(&gds::service::task_manager::Scope::with_handle(
                tokio::runtime::Handle::current(),
                false,
            ))
            .await
            .expect("first window"),
    );
    let mut blocker = Database::builder()
        .build(Sqlite::open(&path))
        .await
        .expect("external connection");
    let transaction = blocker
        .transaction_builder()
        .mode(TransactionMode::Immediate)
        .begin()
        .await
        .expect("hold writer");
    // Reading an existing WAL snapshot needs no writer lock. Reopening the
    // store would instead block on the defaults initialization transaction.
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        storage.load(&gds::service::task_manager::Scope::with_handle(
            tokio::runtime::Handle::current(),
            false,
        )),
    )
    .await
    .expect("new window must not wait for a writer")
    .expect("new window snapshot");
    transaction.rollback().await.expect("release writer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn save_drains_edits_made_while_waiting_even_after_the_window_state_is_dropped() {
    use gds::service::storage::{Database, TransactionMode};
    use toasty_driver_sqlite::Sqlite;
    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().join("state.sqlite");
    let store = Store::open_at(
        &gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false),
        &path,
    )
    .await
    .expect("store");
    let mut places = Places {
        store: Some(Arc::new(store.clone())),
        ..Default::default()
    };
    places.set_view_mode(Path::new("/photos"), ViewMode::Grid);
    let save = places.take_save().expect("pending save");
    let mut blocker = Database::builder()
        .build(Sqlite::open(&path))
        .await
        .expect("second connection");
    let transaction = blocker
        .transaction_builder()
        .mode(TransactionMode::Immediate)
        .begin()
        .await
        .expect("hold writer");
    let mut work = std::pin::pin!(save.run());
    assert!(
        futures_lite::future::poll_once(work.as_mut())
            .await
            .is_none()
    );
    assert!(!places.has_pending());
    places.set_view_mode(Path::new("/photos"), ViewMode::List);
    places.toggle_favorite(Path::new("/documents"));
    drop(places);
    transaction.rollback().await.expect("release writer");
    work.await.expect("save after window release");
    let snapshot = store.read().await.expect("stored snapshot");
    assert_eq!(
        snapshot.effective_view_mode(Path::new("/photos")),
        ViewMode::List
    );
    assert!(snapshot.is_favorite(Path::new("/documents")));
}

#[tokio::test]
async fn tag_locations_persist_as_uris_without_local_paths() {
    use crate::entities::location::scheme::TagAddress;
    let root = tempfile::tempdir().expect("create fixture directory");
    let storage = Storage::new(root.path().join("state.sqlite"));
    let mut places = storage
        .load(&gds::service::task_manager::Scope::with_handle(
            tokio::runtime::Handle::current(),
            false,
        ))
        .await
        .expect("load initial places");
    let location = Location::Tags(TagAddress::new("工作/设计 #1").expect("valid tag address"));
    places.record(location.clone());
    places.set_favorite(location.clone(), true);
    places
        .take_save()
        .expect("pending tag save")
        .run()
        .await
        .expect("save tag location");
    let reopened = storage
        .load(&gds::service::task_manager::Scope::with_handle(
            tokio::runtime::Handle::current(),
            false,
        ))
        .await
        .expect("reload places");
    assert!(reopened.is_favorite(&location));
    assert!(reopened.recent.contains(&location));
    assert!(location.local_path().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn application_shutdown_flushes_each_live_windows_pending_edits() {
    let directory = tempfile::tempdir().expect("fixture");
    let tasks =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);
    let storage = Storage::start(directory.path().join("state.sqlite"), &tasks);
    let mut first = storage.load(&tasks).await.expect("first window");
    let mut second = storage.load(&tasks).await.expect("second window");
    first.toggle_favorite(Path::new("/first"));
    second.toggle_favorite(Path::new("/second"));
    tasks.shutdown().await;
    let snapshot = first
        .store
        .as_ref()
        .expect("shared store")
        .read()
        .await
        .expect("flushed state");
    assert!(snapshot.is_favorite(Path::new("/first")));
    assert!(snapshot.is_favorite(Path::new("/second")));
    assert!(!first.has_pending());
    assert!(!second.has_pending());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_sync_xdg_user_dirs_updates_open_store_and_emits_watch_snapshots() {
    let root = tempfile::tempdir().expect("fixture");
    let home = root.path().join("home");
    let config_dir = root.path().join("config");
    let config_path = config_dir.join("user-dirs.dirs");
    let desktop = home.join("Desktop");
    let downloads = home.join("Downloads");
    let pictures = home.join("Pictures");
    let videos = home.join("Videos");
    let zh_downloads = home.join("下载");
    let zh_videos = home.join("视频");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    std::fs::create_dir_all(&desktop).expect("create Desktop");
    std::fs::create_dir_all(&downloads).expect("create Downloads");
    std::fs::create_dir_all(&pictures).expect("create Pictures");
    std::fs::create_dir_all(&videos).expect("create Videos");
    std::fs::create_dir_all(&zh_downloads).expect("create zh downloads");
    std::fs::create_dir_all(&zh_videos).expect("create zh videos");
    std::fs::write(
        &config_path,
        concat!(
            "XDG_DESKTOP_DIR=\"$HOME/Desktop\"\n",
            "XDG_DOWNLOAD_DIR=\"$HOME/Downloads\"\n",
            "XDG_PICTURES_DIR=\"$HOME/Pictures\"\n",
            "XDG_VIDEOS_DIR=\"$HOME/Videos\"\n",
        ),
    )
    .expect("write initial user-dirs.dirs");

    let tasks =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);
    let source = UserDirsSource {
        home: home.clone(),
        config_path: config_path.clone(),
    };
    let storage =
        Storage::start_with_user_dirs(root.path().join("state.sqlite"), Some(source), &tasks);
    let mut places = storage.load(&tasks).await.expect("load initial places");
    assert_eq!(
        places.quick_access,
        vec![
            Location::from(desktop.as_path()),
            Location::from(downloads.as_path()),
            Location::from(pictures.as_path()),
            Location::from(videos.as_path()),
        ]
    );

    let mut updates = places
        .subscribe_external_updates()
        .expect("loaded places expose external updates");
    let mut storage_updates = storage.subscribe_updates();

    // Queue a local unsaved favorite to verify accepting an external sync rebases pending edits.
    places.set_favorite(Path::new("/pending-favorite").into(), true);

    // Direct sync updates the open store and emits the updated snapshot on the watch channel.
    let relocated_downloads = XdgUserDirs::resolve(
        &home,
        Some(concat!(
            "XDG_DESKTOP_DIR=\"$HOME/Desktop\"\n",
            "XDG_DOWNLOAD_DIR=\"$HOME/下载\"\n",
            "XDG_PICTURES_DIR=\"$HOME/Pictures\"\n",
            "XDG_VIDEOS_DIR=\"$HOME/Videos\"\n",
        )),
    );
    let changed = storage
        .sync_xdg_user_dirs(Some(&home), &relocated_downloads)
        .await
        .expect("sync downloads relocation");
    assert!(changed);
    updates.changed().await.expect("receive direct sync update");
    storage_updates
        .changed()
        .await
        .expect("storage subscriber receives update");
    let snapshot = updates
        .borrow_and_update()
        .clone()
        .expect("snapshot payload");
    let _ = storage_updates.borrow_and_update();
    places.accept(snapshot);
    assert_eq!(
        places.quick_access,
        vec![
            Location::from(desktop.as_path()),
            Location::from(zh_downloads.as_path()),
            Location::from(pictures.as_path()),
            Location::from(videos.as_path()),
        ]
    );
    assert!(places.is_favorite(Path::new("/pending-favorite")));

    // Repeating an identical sync is a no-op and does not emit a spurious update.
    let unchanged = storage
        .sync_xdg_user_dirs(Some(&home), &relocated_downloads)
        .await
        .expect("repeat identical sync");
    assert!(!unchanged);
    assert!(!updates.has_changed().expect("watch channel open"));

    // Editing user-dirs.dirs on disk triggers the live watcher and emits the updated snapshot.
    std::fs::write(
        &config_path,
        concat!(
            "XDG_DESKTOP_DIR=\"$HOME/Desktop\"\n",
            "XDG_DOWNLOAD_DIR=\"$HOME/下载\"\n",
            "XDG_PICTURES_DIR=\"$HOME/Pictures\"\n",
            "XDG_VIDEOS_DIR=\"$HOME/视频\"\n",
        ),
    )
    .expect("update user-dirs.dirs on disk");
    tokio::time::timeout(Duration::from_secs(5), updates.changed())
        .await
        .expect("live watcher emits update within timeout")
        .expect("watch channel remains open");
    let live_snapshot = updates
        .borrow_and_update()
        .clone()
        .expect("live snapshot payload");
    places.accept(live_snapshot);
    assert_eq!(
        places.quick_access,
        vec![
            Location::from(desktop.as_path()),
            Location::from(zh_downloads.as_path()),
            Location::from(pictures.as_path()),
            Location::from(zh_videos.as_path()),
        ]
    );
    assert!(places.is_favorite(Path::new("/pending-favorite")));

    tasks.shutdown().await;
}
