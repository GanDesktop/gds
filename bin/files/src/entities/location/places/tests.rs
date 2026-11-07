use super::*;

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
