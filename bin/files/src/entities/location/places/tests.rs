use super::*;

#[test]
fn committed_snapshot_rebases_pending_local_edits() {
    let mut places = Places::default();
    places.set_view_mode(Path::new("/photos"), ViewMode::Grid);
    let mut saved = Snapshot::default();
    saved.set_columns(Path::new("/photos"), Layout::default());
    saved.favorites.push("/external".into());
    places.accept(saved);
    assert!(places.is_favorite(Path::new("/external")));
    assert!(places.columns_for(Path::new("/photos")).is_some());
    assert_eq!(
        places.effective_view_mode(Path::new("/photos")),
        ViewMode::Grid
    );
    assert!(places.has_pending());
}

#[tokio::test]
async fn refreshing_favorites_includes_external_edits_and_keeps_pending_local_edits() {
    let root = tempfile::tempdir().expect("fixture");
    let store = Store::open_at(&root.path().join("state.sqlite"))
        .await
        .expect("store");
    let mut places = Places {
        store: Some(Arc::new(store.clone())),
        ..Default::default()
    };
    places.toggle_favorite(Path::new("/local"));
    store
        .save(vec![Change::Favorite {
            location: "/external".into(),
            present: true,
        }])
        .await
        .expect("external edit");
    let snapshot = places
        .reload()
        .expect("existing store")
        .await
        .expect("reload");
    places.refresh_favorites(snapshot);
    assert!(places.is_favorite(Path::new("/local")));
    assert!(places.is_favorite(Path::new("/external")));
    places
        .take_save()
        .expect("pending local edit")
        .run()
        .await
        .expect("save");
    let saved = store.read().await.expect("stored favorites");
    assert!(saved.is_favorite(Path::new("/local")));
    assert!(saved.is_favorite(Path::new("/external")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn save_drains_edits_made_while_waiting_even_after_the_window_state_is_dropped() {
    use gds::service::storage::{Database, TransactionMode};
    use toasty_driver_sqlite::Sqlite;
    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().join("state.sqlite");
    let store = Store::open_at(&path).await.expect("store");
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
async fn failed_batch_keeps_edits_available_for_retry() {
    use gds::service::storage::Database;
    use toasty_driver_sqlite::Sqlite;
    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().join("state.sqlite");
    let store = Store::open_at(&path).await.expect("store");
    let mut places = Places {
        store: Some(Arc::new(store.clone())),
        ..Default::default()
    };
    places.toggle_favorite(Path::new("/documents"));
    places.set_view_mode(Path::new("/photos"), ViewMode::Grid);
    let mut db = Database::builder()
        .build(Sqlite::open(&path))
        .await
        .expect("second connection");
    toasty::sql::statement("CREATE TRIGGER fail_view BEFORE INSERT ON view_settings BEGIN SELECT RAISE(FAIL, 'fixture failure'); END;")
        .exec(&mut db).await.expect("fail writes");
    assert!(places.take_save().expect("save").run().await.is_err());
    assert!(places.has_pending());
    assert!(
        store
            .read()
            .await
            .expect("rolled back snapshot")
            .favorites
            .is_empty()
    );
    toasty::sql::statement("DROP TRIGGER fail_view")
        .exec(&mut db)
        .await
        .expect("allow writes");
    let snapshot = places
        .take_save()
        .expect("retry")
        .run()
        .await
        .expect("saved");
    places.accept(snapshot);
    assert!(!places.has_pending());
    assert!(places.is_favorite(Path::new("/documents")));
    assert_eq!(
        places.effective_view_mode(Path::new("/photos")),
        ViewMode::Grid
    );
}

#[tokio::test]
async fn shutdown_flushes_pending_edits_and_closed_idle_windows_need_no_work() {
    let root = tempfile::tempdir().expect("fixture");
    let store = Store::open_at(&root.path().join("state.sqlite"))
        .await
        .expect("store");
    let mut places = Places {
        store: Some(Arc::new(store.clone())),
        ..Default::default()
    };
    let shutdown = places.shutdown().expect("shutdown handle");
    places.set_view_mode(Path::new("/photos"), ViewMode::Grid);
    shutdown.clone().run().await.expect("flush on quit");
    assert!(!places.has_pending());
    assert_eq!(
        store
            .read()
            .await
            .expect("saved state")
            .effective_view_mode(Path::new("/photos")),
        ViewMode::Grid
    );
    drop(places);
    shutdown.run().await.expect("already released");
}
