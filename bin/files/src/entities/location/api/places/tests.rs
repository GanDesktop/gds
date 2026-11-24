use super::*;
use crate::entities::file::x::location::{Layout, Sort};
use crate::entities::location::Location as Address;

#[tokio::test]
async fn persist_ordered_abstract_locations() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("state.sqlite");
    let local = Address::from(Path::new("/existing-pin"));
    let tags = Address::from_uri("tags:/工作/设计").expect("parse tag address");
    let remote =
        Address::from_uri("sftp://user@example.com/home/user").expect("parse remote address");
    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);

    let store = Store::open_at(&scope, &path)
        .await
        .expect("open places store");
    store
        .save(vec![
            Change::Pin {
                location: local.clone(),
                present: true,
            },
            Change::Pin {
                location: tags.clone(),
                present: true,
            },
            Change::Pin {
                location: Address::Recent,
                present: true,
            },
            Change::Pin {
                location: remote.clone(),
                present: true,
            },
        ])
        .await
        .expect("save initial pins");
    assert_eq!(
        store.read().await.expect("read initial pins").pinned,
        vec![local.clone(), tags.clone(), Address::Recent, remote.clone()]
    );

    // A stale window only reorders its own pins; a newer remote pin survives.
    let expected = vec![Address::Recent, tags.clone(), local.clone(), remote.clone()];
    store
        .save(vec![Change::PinOrder(expected[..3].to_vec())])
        .await
        .expect("save reordered pins");
    drop(store);

    let store = Store::open_at(&scope, &path)
        .await
        .expect("reopen places store");
    assert_eq!(
        store.read().await.expect("read reordered pins").pinned,
        expected
    );
    store
        .save(vec![Change::Pin {
            location: tags,
            present: false,
        }])
        .await
        .expect("remove tag pin");
    assert_eq!(
        store.read().await.expect("read pins after removal").pinned,
        vec![Address::Recent, local, remote]
    );
}

#[tokio::test]
async fn read_legacy_pascal_case_view_settings() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("state.sqlite");
    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);

    let mut store = Store::open_at(&scope, &path)
        .await
        .expect("open places store");
    toasty::sql::query(
        r#"UPDATE view_settings SET columns = '["Name","Modified","DetailedType"]', sort = '{"key":"Name","direction":"Ascending"}' WHERE key_kind = 'defaults'"#,
    )
    .exec(&mut store.db)
    .await
    .expect("write legacy view_settings JSON");
    drop(store);

    let store = Store::open_at(&scope, &path)
        .await
        .expect("reopen places store with legacy view_settings");
    let places = store
        .read()
        .await
        .expect("read places snapshot with legacy view_settings");
    let expected_columns: Layout =
        serde_json::from_str(r#"["name","modified","detailed-type"]"#).expect("canonical layout");
    assert_eq!(places.defaults.columns, Some(expected_columns));
    assert_eq!(places.defaults.sort, Some(Sort::default()));
}

#[tokio::test]
async fn quick_access_seeds_once_reorders_and_stays_empty_when_cleared() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("state.sqlite");
    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);
    let desktop = Address::from(Path::new("/home/user/Desktop"));
    let downloads = Address::from(Path::new("/home/user/Downloads"));
    let pictures = Address::from(Path::new("/home/user/Pictures"));
    let videos = Address::from(Path::new("/home/user/Videos"));
    let projects = Address::from(Path::new("/home/user/Projects"));
    let defaults = vec![
        desktop.clone(),
        downloads.clone(),
        pictures.clone(),
        videos.clone(),
    ];

    let store = Store::open_at_with_quick_access(&scope, &path, &defaults)
        .await
        .expect("open store with seeded quick access");
    assert_eq!(
        store.read().await.expect("read seeded").quick_access,
        defaults
    );

    // Adding an existing item is idempotent; adding a new item appends at end.
    store
        .save(vec![
            Change::QuickAccess {
                location: downloads.clone(),
                present: true,
            },
            Change::QuickAccess {
                location: projects.clone(),
                present: true,
            },
        ])
        .await
        .expect("append quick access");
    assert_eq!(
        store.read().await.expect("read appended").quick_access,
        vec![
            desktop.clone(),
            downloads.clone(),
            pictures.clone(),
            videos.clone(),
            projects.clone(),
        ]
    );

    // Reordering persists across reopen.
    let reordered = vec![
        projects.clone(),
        downloads.clone(),
        desktop.clone(),
        videos.clone(),
        pictures.clone(),
    ];
    store
        .save(vec![Change::QuickAccessOrder(reordered.clone())])
        .await
        .expect("save quick access order");
    drop(store);

    let store = Store::open_at_with_quick_access(&scope, &path, &defaults)
        .await
        .expect("reopen store after reorder");
    assert_eq!(
        store.read().await.expect("read reordered").quick_access,
        reordered
    );

    // Removing every item must NOT cause defaults to be re-seeded on next launch.
    let remove_all: Vec<Change> = reordered
        .into_iter()
        .map(|location| Change::QuickAccess {
            location,
            present: false,
        })
        .collect();
    store
        .save(remove_all)
        .await
        .expect("clear all quick access");
    assert!(
        store
            .read()
            .await
            .expect("read empty")
            .quick_access
            .is_empty()
    );
    drop(store);

    let store = Store::open_at_with_quick_access(&scope, &path, &defaults)
        .await
        .expect("reopen cleared store");
    assert!(
        store
            .read()
            .await
            .expect("read after reopen")
            .quick_access
            .is_empty(),
        "clearing all quick access folders must not re-seed defaults on next launch"
    );
}

#[tokio::test]
async fn migration_v1_to_v2_preserves_saved_locations_and_seeds_quick_access_once() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("legacy.sqlite");
    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);

    // Open a v1-only database and populate saved_locations.
    const V1_MIGRATIONS: MigrationSet = MigrationSet::new(&[MigrationFile::new(
        1,
        "001-places.sql",
        include_str!("assets/001-places.sql"),
    )]);
    let mut db = Options::new(V1_MIGRATIONS)
        .models(models!(SavedLocation, ViewSettings))
        .open(&scope, &path)
        .await
        .expect("open v1 database");
    let existing_pin = Address::from(Path::new("/legacy-pin"));
    let key = record::ViewKey::new(&crate::entities::location::ViewScope::Location(
        existing_pin.clone(),
    ))
    .expect("encode legacy pin");
    SavedLocation::upsert_by_kind_and_location_kind_and_identity_and_path(
        record::Kind::Pin,
        key.kind,
        key.identity,
        key.path,
    )
    .position(0)
    .exec(&mut db)
    .await
    .expect("insert legacy pin");
    drop(db);

    // Upgrade to v2 and verify legacy pin survives while quick_access is seeded once.
    let defaults = vec![
        Address::from(Path::new("/home/user/Desktop")),
        Address::from(Path::new("/home/user/Downloads")),
    ];
    let store = Store::open_at_with_quick_access(&scope, &path, &defaults)
        .await
        .expect("upgrade v1 store to v2");
    let snapshot = store.read().await.expect("read upgraded store");
    assert_eq!(snapshot.pinned, vec![existing_pin]);
    assert_eq!(snapshot.quick_access, defaults);
}

#[test]
fn resolve_default_quick_access_handles_xdg_fallbacks_and_skips_home_or_missing() {
    let root = tempfile::tempdir().expect("create fixture home");
    let home = root.path();
    let desktop = home.join("Desktop");
    let downloads = home.join("Downloads");
    let pictures = home.join("Pictures");
    let custom_videos = home.join("媒体视频");
    for dir in [&desktop, &downloads, &pictures, &custom_videos] {
        std::fs::create_dir_all(dir).expect("create dir");
    }

    // Without user-dirs.dirs, fallback names resolve existing Desktop/Downloads/Pictures and skip missing Videos.
    assert_eq!(
        resolve_default_quick_access(home, [None, None, None, None], true),
        vec![
            Address::from(desktop.as_path()),
            Address::from(downloads.as_path()),
            Address::from(pictures.as_path()),
        ]
    );

    // With XDG user-dirs.dirs: disabled entries pointing to $HOME are skipped; custom localized paths work.
    assert_eq!(
        resolve_default_quick_access(
            home,
            [
                Some(home),
                Some(downloads.as_path()),
                Some(pictures.as_path()),
                Some(custom_videos.as_path()),
            ],
            false,
        ),
        vec![
            Address::from(downloads.as_path()),
            Address::from(pictures.as_path()),
            Address::from(custom_videos.as_path()),
        ]
    );
}
