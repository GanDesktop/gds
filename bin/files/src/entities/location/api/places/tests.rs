use super::super::super::model::places::RECENT_LIMIT;
use super::*;

#[tokio::test]
async fn mixed_places_share_visit_order_and_bookmarks_survive_clear_and_reopen() {
    let root = tempfile::tempdir().expect("fixture");
    let store = open(root.path()).await;
    let local = Location::from(Path::new("/photos"));
    let remote = Location::from_uri("sftp://alice@host:2222/team").expect("remote");
    let virtual_location = Location::Network;
    let places = save(
        &store,
        vec![
            Change::Recent(remote.clone()),
            Change::Favorite {
                location: remote.clone(),
                present: true,
            },
            Change::Recent(local.clone()),
            Change::Recent(virtual_location.clone()),
            Change::Favorite {
                location: virtual_location.clone(),
                present: true,
            },
            Change::Recent(remote.clone()),
        ],
    )
    .await;
    assert_eq!(
        places.recent,
        [remote.clone(), virtual_location.clone(), local]
    );
    let reopened = open(root.path()).await;
    let loaded = reopened.read().await.expect("reopen");
    assert_eq!(loaded.recent, places.recent);
    assert_eq!(loaded.favorites, [remote.clone(), virtual_location]);
    assert!(Location::from_uri("sftp://alice:secret@host/team").is_err());
    let cleared = save(&reopened, vec![Change::ClearRecent]).await;
    assert!(cleared.recent.is_empty());
    assert_eq!(cleared.favorites, loaded.favorites);
    let revisited = save(&reopened, vec![Change::Recent(remote.clone())]).await;
    assert_eq!(revisited.recent, [remote.clone()]);
    assert!(revisited.is_favorite(remote));
}
use crate::entities::file::x::location::{Layout, ViewMode};
use crate::entities::location::{Location, ViewScope};
use std::ffi::OsString;
use toasty_driver_sqlite::Sqlite;

async fn open(root: &Path) -> Store {
    Store::open_at(&root.join("state.sqlite"))
        .await
        .expect("places store")
}

async fn save(store: &Store, changes: Vec<Change>) -> Places {
    store.save(changes).await.expect("save changes")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_a_locked_database_yields_and_preserves_saved_places() {
    let root = tempfile::tempdir().expect("fixture");
    let path = root.path().join("state.sqlite");
    let store = Store::open_at(&path).await.expect("store");
    store
        .save(vec![Change::Favorite {
            location: "/photos".into(),
            present: true,
        }])
        .await
        .expect("saved favorite");
    let mut db = Database::builder()
        .build(Sqlite::open(&path))
        .await
        .expect("second connection");
    let lock = db
        .transaction_builder()
        .mode(storage::TransactionMode::Immediate)
        .begin()
        .await
        .expect("hold writer");
    let mut opening = std::pin::pin!(Store::open_at(&path));
    assert!(
        futures_lite::future::poll_once(opening.as_mut())
            .await
            .is_none()
    );
    lock.rollback().await.expect("release writer");
    let reopened = opening.await.expect("open after unlock");
    assert!(
        reopened
            .read()
            .await
            .expect("saved places")
            .is_favorite(Path::new("/photos"))
    );
}

#[tokio::test]
async fn reopening_preserves_raw_paths_and_directory_views() {
    let root = tempfile::tempdir().expect("fixture");
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(b"/tmp/caf\xe9".to_vec())
    };
    #[cfg(not(unix))]
    let path = OsString::from("/tmp/photos");
    let layout = Layout::default();
    let store = open(root.path()).await;
    save(
        &store,
        vec![
            Change::Favorite {
                location: Location::Directory(path.clone()),
                present: true,
            },
            Change::Pin {
                path: path.clone(),
                present: true,
            },
            Change::Recent(path.clone().into()),
            Change::Columns {
                scope: ViewScope::Location(Location::Directory(path.clone())),
                layout: Some(layout.clone()),
            },
            Change::ViewMode {
                location: Location::Directory(path.clone()),
                mode: ViewMode::Grid,
            },
        ],
    )
    .await;
    let snapshot = open(root.path()).await.read().await.expect("reopen");
    assert_eq!(snapshot.favorites, [Location::Directory(path.clone())]);
    assert_eq!(snapshot.recent, [Location::Directory(path.clone())]);
    assert_eq!(snapshot.pinned, [path.clone()]);
    assert_eq!(snapshot.columns_for(Path::new(&path)), Some(&layout));
    assert_eq!(
        snapshot.effective_view_mode(Path::new(&path)),
        ViewMode::Grid
    );
}

#[tokio::test]
async fn independent_windows_merge_record_edits_and_column_reset_keeps_view_mode() {
    let root = tempfile::tempdir().expect("fixture");
    let first = open(root.path()).await;
    let second = open(root.path()).await;
    let photos = OsString::from("/photos");
    let docs = OsString::from("/documents");
    // Both windows started with an empty snapshot.
    save(
        &first,
        vec![
            Change::ViewMode {
                location: Location::Directory(photos.clone()),
                mode: ViewMode::Grid,
            },
            Change::Favorite {
                location: Location::Directory(photos.clone()),
                present: true,
            },
            Change::Pin {
                path: photos.clone(),
                present: true,
            },
        ],
    )
    .await;
    let snapshot = save(
        &second,
        vec![
            Change::ViewMode {
                location: Location::Directory(docs.clone()),
                mode: ViewMode::List,
            },
            Change::Favorite {
                location: Location::Directory(docs.clone()),
                present: true,
            },
            Change::Columns {
                scope: ViewScope::Location(Location::Directory(photos.clone())),
                layout: Some(Layout::default()),
            },
        ],
    )
    .await;
    assert_eq!(
        snapshot.favorites,
        [
            Location::Directory(photos.clone()),
            Location::Directory(docs.clone())
        ]
    );
    assert_eq!(snapshot.pinned, [photos.clone()]);
    assert_eq!(
        snapshot.effective_view_mode(Path::new(&photos)),
        ViewMode::Grid
    );
    let snapshot = save(
        &first,
        vec![Change::Columns {
            scope: ViewScope::Location(Location::Directory(photos.clone())),
            layout: None,
        }],
    )
    .await;
    assert!(snapshot.columns_for(Path::new(&photos)).is_none());
    assert_eq!(
        snapshot.effective_view_mode(Path::new(&photos)),
        ViewMode::Grid
    );
    assert_eq!(
        snapshot.effective_view_mode(Path::new(&docs)),
        ViewMode::List
    );
    let reopened = open(root.path()).await.read().await.expect("reload");
    assert_eq!(reopened.favorites, snapshot.favorites);
    assert_eq!(reopened.views, snapshot.views);
}

#[tokio::test]
async fn recent_records_are_ordered_deduplicated_bounded_and_clearable() {
    let root = tempfile::tempdir().expect("fixture");
    let store = open(root.path()).await;
    let changes = (0..=RECENT_LIMIT)
        .map(|index| Change::Recent(Location::from(Path::new(&format!("/{index}")))))
        .collect();
    let snapshot = save(&store, changes).await;
    assert_eq!(snapshot.recent.len(), RECENT_LIMIT);
    assert!(!snapshot.recent.contains(&Location::from("/0")));
    let snapshot = save(&store, vec![Change::Recent("/1".into())]).await;
    assert_eq!(snapshot.recent.len(), RECENT_LIMIT);
    assert_eq!(snapshot.recent[0], Location::from("/1"));
    assert!(
        save(&store, vec![Change::ClearRecent])
            .await
            .recent
            .is_empty()
    );
}

#[tokio::test]
async fn location_settings_survive_restart_and_independent_edits_preserve_other_fields() {
    use crate::entities::file::columns::Column;
    use crate::entities::file::sorting::{Direction, Key, Sort};
    use crate::entities::location::VolumeIdentity;
    let root = tempfile::tempdir().expect("fixture");
    let first = open(root.path()).await;
    let second = open(root.path()).await;
    let folder = Location::from(Path::new("/photos"));
    let trash = Location::Trash;
    let volume = Location::Volume {
        identity: VolumeIdentity::Stable("photo-disk".into()),
        relative: "pictures".into(),
    };
    let defaults = Layout::new([Column::Name, Column::Size]);
    let custom = Layout::new([Column::Name, Column::Modified]);
    let sort = Sort {
        key: Key::Modified,
        direction: Direction::Descending,
    };
    save(
        &first,
        vec![
            Change::Columns {
                scope: ViewScope::Defaults,
                layout: Some(defaults.clone()),
            },
            Change::Columns {
                scope: ViewScope::Location(folder.clone()),
                layout: Some(custom.clone()),
            },
            Change::Widths {
                location: folder.clone(),
                widths: [("name".into(), 320.)].into(),
            },
            Change::Sort {
                location: folder.clone(),
                sort,
            },
            Change::Details {
                location: folder.clone(),
                visible: true,
            },
            Change::Details {
                location: trash.clone(),
                visible: true,
            },
            Change::Details {
                location: volume.clone(),
                visible: true,
            },
        ],
    )
    .await;
    save(
        &second,
        vec![
            Change::Widths {
                location: folder.clone(),
                widths: [("modified".into(), 190.)].into(),
            },
            Change::ViewMode {
                location: folder.clone(),
                mode: ViewMode::Grid,
            },
        ],
    )
    .await;
    let reopened = open(root.path()).await;
    let snapshot = reopened.read().await.expect("reopen settings");
    assert_eq!(snapshot.effective_columns(&folder), &custom);
    assert_eq!(snapshot.effective_columns(Path::new("/other")), &defaults);
    assert_eq!(
        snapshot.column_widths(&folder),
        [("name".into(), 320.), ("modified".into(), 190.)].into()
    );
    assert_eq!(snapshot.effective_sort(&folder), sort);
    assert_eq!(snapshot.effective_view_mode(&folder), ViewMode::Grid);
    assert!(snapshot.details_visible(&folder));
    assert!(snapshot.details_visible(&trash));
    assert!(snapshot.details_visible(&volume));
    assert!(!snapshot.details_visible(Location::Favorites));
    assert_eq!(
        snapshot.effective_view_mode(Path::new("/new")),
        ViewMode::List
    );
    let reset = save(
        &reopened,
        vec![Change::Columns {
            scope: ViewScope::Location(folder.clone()),
            layout: None,
        }],
    )
    .await;
    assert_eq!(reset.effective_columns(&folder), &defaults);
    assert_eq!(
        reset.column_widths(&folder),
        snapshot.column_widths(&folder)
    );
    assert_eq!(reset.effective_sort(&folder), sort);
    assert!(reset.details_visible(&folder));
}
