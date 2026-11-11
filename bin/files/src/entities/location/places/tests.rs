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
