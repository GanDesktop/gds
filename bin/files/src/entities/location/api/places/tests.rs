use super::*;
use crate::entities::file::x::location::{Layout, Sort};
use crate::entities::location::Location as Address;

#[tokio::test]
async fn persist_ordered_abstract_locations() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("state.sqlite");
    let local = Address::from(Path::new("/existing-pin"));
    let tags =
        crate::entities::location::scheme::parse("tags:/工作/设计").expect("parse tag address");
    let remote = crate::entities::location::scheme::parse("sftp://user@example.com/home/user")
        .expect("parse remote address");
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

#[tokio::test]
async fn migration_heals_pre_squash_databases_with_legacy_ids_1_through_5() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("presquash.sqlite");
    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);

    // Simulate a database migrated through historical 001..005 before the squash:
    // `saved_locations` and `view_settings` exist, `__toasty_migrations` has ids 1..5,
    // and neither `quick_access` nor `places_meta` exists yet.
    const V1_MIGRATIONS: MigrationSet = MigrationSet::new(&[MigrationFile::new(
        1,
        "001-places.sql",
        include_str!("assets/001-places.sql"),
    )]);
    let mut db = Options::new(V1_MIGRATIONS)
        .models(models!(SavedLocation, ViewSettings))
        .open(&scope, &path)
        .await
        .expect("open pre-squash database");
    for (id, name) in [
        (2, "002-location-views.sql"),
        (3, "003-remote-locations.sql"),
        (4, "004-unified-places.sql"),
        (5, "005-location-pins.sql"),
    ] {
        toasty::sql::query(format!(
            "INSERT INTO __toasty_migrations (id, name, applied_at) VALUES ({id}, '{name}', '2026-11-10 00:00:00')"
        ))
        .exec(&mut db)
        .await
        .expect("seed legacy migration row");
    }
    let existing_pin = Address::from(Path::new("/pre-squash-pin"));
    let key = record::ViewKey::new(&crate::entities::location::ViewScope::Location(
        existing_pin.clone(),
    ))
    .expect("encode pre-squash pin");
    SavedLocation::upsert_by_kind_and_location_kind_and_identity_and_path(
        record::Kind::Pin,
        key.kind,
        key.identity,
        key.path,
    )
    .position(0)
    .exec(&mut db)
    .await
    .expect("insert pre-squash pin");
    drop(db);

    let defaults = vec![Address::from(Path::new("/home/user/Desktop"))];
    let store = Store::open_at_with_quick_access(&scope, &path, &defaults)
        .await
        .expect("upgrade pre-squash store");
    let snapshot = store.read().await.expect("read healed store");
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
    std::fs::create_dir_all(&desktop).expect("create Desktop");
    std::fs::create_dir_all(&downloads).expect("create Downloads");
    std::fs::create_dir_all(&pictures).expect("create Pictures");
    std::fs::create_dir_all(&custom_videos).expect("create custom videos");

    // Without user-dirs.dirs, fallback names resolve existing Desktop/Downloads/Pictures and skip missing Videos.
    assert_eq!(
        XdgUserDirs::resolve(home, None).initial_quick_access_locations(),
        vec![
            Address::from(desktop.as_path()),
            Address::from(downloads.as_path()),
            Address::from(pictures.as_path()),
        ]
    );

    // With XDG user-dirs.dirs: disabled entries pointing to $HOME are skipped; custom localized paths work.
    let config = r#"
# User directories
XDG_DESKTOP_DIR="$HOME"
XDG_DOWNLOAD_DIR="$HOME/Downloads"
XDG_PICTURES_DIR="$HOME/Pictures"
XDG_VIDEOS_DIR="$HOME/媒体视频"
"#;
    assert_eq!(
        XdgUserDirs::resolve(home, Some(config)).initial_quick_access_locations(),
        vec![
            Address::from(downloads.as_path()),
            Address::from(pictures.as_path()),
            Address::from(custom_videos.as_path()),
        ]
    );
}

#[test]
fn resolve_config_path_follows_xdg_base_dir_spec() {
    use std::ffi::OsStr;

    let home = Path::new("/home/alice");
    assert_eq!(
        user_dirs::resolve_config_path(Some(home), Some(OsStr::new("/etc/custom-config"))),
        Some(std::path::PathBuf::from(
            "/etc/custom-config/user-dirs.dirs"
        ))
    );
    assert_eq!(
        user_dirs::resolve_config_path(Some(home), Some(OsStr::new("relative/config"))),
        Some(std::path::PathBuf::from(
            "/home/alice/.config/user-dirs.dirs"
        ))
    );
    assert_eq!(
        user_dirs::resolve_config_path(Some(home), Some(OsStr::new(""))),
        Some(std::path::PathBuf::from(
            "/home/alice/.config/user-dirs.dirs"
        ))
    );
    assert_eq!(
        user_dirs::resolve_config_path(Some(home), None),
        Some(std::path::PathBuf::from(
            "/home/alice/.config/user-dirs.dirs"
        ))
    );
    assert_eq!(user_dirs::resolve_config_path(None, None), None);
}

#[test]
fn parse_user_dirs_handles_home_expansion_absolute_escapes_utf8_and_disabled() {
    use user_dirs::XdgUserDirSlot;

    let home = Path::new("/home/alice");
    let config = r#"
# FreeDesktop user-dirs.dirs
XDG_DESKTOP_DIR="$HOME/"
XDG_DOWNLOAD_DIR="$HOME/下载" # trailing comment
XDG_DOCUMENTS_DIR="$HOME/My \"Docs\" \\ \$1"
XDG_PICTURES_DIR="$HOME/图片"
XDG_MUSIC_DIR="/mnt/audio/Music/"
XDG_VIDEOS_DIR="/home/alice"
XDG_PUBLICSHARE_DIR="$HOME/Public"

# Malformed or relative assignments must be ignored without clobbering valid slots.
XDG_DOWNLOAD_DIR="Downloads"
XDG_PICTURES_DIR="./Pictures"
XDG_MUSIC_DIR="\$HOME/Music"
XDG_PUBLICSHARE_DIR="$HOMEDIR/Public"
XDG_DOCUMENTS_DIR="$HOME/unterminated
XDG_UNKNOWN_DIR="$HOME/Ignored"
"#;

    let resolved = XdgUserDirs::resolve(home, Some(config));
    assert_eq!(resolved.get(XdgUserDirSlot::Desktop), None);
    assert_eq!(
        resolved.get(XdgUserDirSlot::Downloads),
        Some(Path::new("/home/alice/下载"))
    );
    assert_eq!(
        resolved.get(XdgUserDirSlot::Documents),
        Some(Path::new(r#"/home/alice/My "Docs" \ $1"#))
    );
    assert_eq!(
        resolved.get(XdgUserDirSlot::Pictures),
        Some(Path::new("/home/alice/图片"))
    );
    assert_eq!(
        resolved.get(XdgUserDirSlot::Music),
        Some(Path::new("/mnt/audio/Music"))
    );
    assert_eq!(resolved.get(XdgUserDirSlot::Videos), None);
    assert_eq!(
        resolved.get(XdgUserDirSlot::Public),
        Some(Path::new("/home/alice/Public"))
    );
}

#[tokio::test]
async fn xdg_user_dirs_relocation_updates_quick_access_in_place_preserving_order_and_custom_pins() {
    use user_dirs::XdgUserDirSlot;

    let root = tempfile::tempdir().expect("create fixture directory");
    let home = root.path().join("home");
    let db_path = root.path().join("state.sqlite");
    let desktop = home.join("Desktop");
    let downloads = home.join("Downloads");
    let pictures = home.join("Pictures");
    let videos = home.join("Videos");
    let projects = home.join("Projects");
    let zh_downloads = home.join("下载");
    let zh_pictures = home.join("图片");
    std::fs::create_dir_all(&desktop).expect("create Desktop");
    std::fs::create_dir_all(&downloads).expect("create Downloads");
    std::fs::create_dir_all(&pictures).expect("create Pictures");
    std::fs::create_dir_all(&videos).expect("create Videos");
    std::fs::create_dir_all(&projects).expect("create Projects");
    std::fs::create_dir_all(&zh_downloads).expect("create zh downloads");
    std::fs::create_dir_all(&zh_pictures).expect("create zh pictures");

    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);
    let initial = XdgUserDirs::from_slots([
        (XdgUserDirSlot::Desktop, Some(desktop.clone())),
        (XdgUserDirSlot::Downloads, Some(downloads.clone())),
        (XdgUserDirSlot::Pictures, Some(pictures.clone())),
        (XdgUserDirSlot::Videos, Some(videos.clone())),
    ]);

    let store = Store::open_at_with_xdg_dirs(&scope, &db_path, Some(&home), &initial)
        .await
        .expect("open store with initial XDG dirs");
    let reordered = vec![
        Address::from(projects.as_path()),
        Address::from(downloads.as_path()),
        Address::from(desktop.as_path()),
        Address::from(videos.as_path()),
        Address::from(pictures.as_path()),
    ];
    store
        .save(vec![
            Change::QuickAccess {
                location: Address::from(projects.as_path()),
                present: true,
            },
            Change::QuickAccessOrder(reordered),
        ])
        .await
        .expect("add custom pin and reorder quick access");
    drop(store);

    // Relocate Downloads and Pictures across reopen: entries update at their exact positions.
    let relocated = XdgUserDirs::from_slots([
        (XdgUserDirSlot::Desktop, Some(desktop.clone())),
        (XdgUserDirSlot::Downloads, Some(zh_downloads.clone())),
        (XdgUserDirSlot::Pictures, Some(zh_pictures.clone())),
        (XdgUserDirSlot::Videos, Some(videos.clone())),
    ]);
    let store = Store::open_at_with_xdg_dirs(&scope, &db_path, Some(&home), &relocated)
        .await
        .expect("reopen store with relocated XDG dirs");
    assert_eq!(
        store.read().await.expect("read relocated").quick_access,
        vec![
            Address::from(projects.as_path()),
            Address::from(zh_downloads.as_path()),
            Address::from(desktop.as_path()),
            Address::from(videos.as_path()),
            Address::from(zh_pictures.as_path()),
        ]
    );

    // Relocating Videos to an already-present custom pin removes stale Videos without duplicating.
    let deduplicated = XdgUserDirs::from_slots([
        (XdgUserDirSlot::Desktop, Some(desktop.clone())),
        (XdgUserDirSlot::Downloads, Some(zh_downloads.clone())),
        (XdgUserDirSlot::Pictures, Some(zh_pictures.clone())),
        (XdgUserDirSlot::Videos, Some(projects.clone())),
    ]);
    let sync = store
        .sync_xdg_user_dirs(Some(&home), &deduplicated)
        .await
        .expect("sync deduplicated relocation");
    assert!(sync.changed);
    assert_eq!(
        sync.places.quick_access,
        vec![
            Address::from(projects.as_path()),
            Address::from(zh_downloads.as_path()),
            Address::from(desktop.as_path()),
            Address::from(zh_pictures.as_path()),
        ]
    );
}

#[tokio::test]
async fn xdg_user_dirs_sync_does_not_readd_removed_slot_and_keeps_cleared_list_empty() {
    use user_dirs::XdgUserDirSlot;

    let root = tempfile::tempdir().expect("create fixture directory");
    let home = root.path().join("home");
    let db_path = root.path().join("state.sqlite");
    let desktop = home.join("Desktop");
    let downloads = home.join("Downloads");
    let zh_downloads = home.join("下载");
    let zh_desktop = home.join("桌面");
    std::fs::create_dir_all(&desktop).expect("create Desktop");
    std::fs::create_dir_all(&downloads).expect("create Downloads");
    std::fs::create_dir_all(&zh_downloads).expect("create zh downloads");
    std::fs::create_dir_all(&zh_desktop).expect("create zh desktop");

    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);
    let initial = XdgUserDirs::from_slots([
        (XdgUserDirSlot::Desktop, Some(desktop.clone())),
        (XdgUserDirSlot::Downloads, Some(downloads.clone())),
    ]);
    let store = Store::open_at_with_xdg_dirs(&scope, &db_path, Some(&home), &initial)
        .await
        .expect("open store");

    // Explicitly remove Downloads; relocating XDG_DOWNLOAD_DIR later must not re-add it.
    store
        .save(vec![Change::QuickAccess {
            location: Address::from(downloads.as_path()),
            present: false,
        }])
        .await
        .expect("remove Downloads");

    let after_download_relocation = XdgUserDirs::from_slots([
        (XdgUserDirSlot::Desktop, Some(desktop.clone())),
        (XdgUserDirSlot::Downloads, Some(zh_downloads.clone())),
    ]);
    let sync = store
        .sync_xdg_user_dirs(Some(&home), &after_download_relocation)
        .await
        .expect("sync after removing Downloads");
    assert!(!sync.changed);
    assert_eq!(
        sync.places.quick_access,
        vec![Address::from(desktop.as_path())]
    );

    // Clear the remaining item; reopening after another relocation must keep Quick Access empty.
    store
        .save(vec![Change::QuickAccess {
            location: Address::from(desktop.as_path()),
            present: false,
        }])
        .await
        .expect("clear remaining quick access");
    drop(store);

    let after_desktop_relocation = XdgUserDirs::from_slots([
        (XdgUserDirSlot::Desktop, Some(zh_desktop)),
        (XdgUserDirSlot::Downloads, Some(zh_downloads)),
    ]);
    let store =
        Store::open_at_with_xdg_dirs(&scope, &db_path, Some(&home), &after_desktop_relocation)
            .await
            .expect("reopen emptied store");
    assert!(
        store
            .read()
            .await
            .expect("read emptied store")
            .quick_access
            .is_empty()
    );
}

#[tokio::test]
async fn xdg_user_dirs_disabled_slot_removes_only_missing_directories() {
    use user_dirs::XdgUserDirSlot;

    let root = tempfile::tempdir().expect("create fixture directory");
    let home = root.path().join("home");
    let db_path = root.path().join("state.sqlite");
    let pictures = home.join("Pictures");
    let videos = home.join("Videos");
    std::fs::create_dir_all(&pictures).expect("create Pictures");
    std::fs::create_dir_all(&videos).expect("create Videos");

    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);
    let initial = XdgUserDirs::from_slots([
        (XdgUserDirSlot::Pictures, Some(pictures.clone())),
        (XdgUserDirSlot::Videos, Some(videos.clone())),
    ]);
    let store = Store::open_at_with_xdg_dirs(&scope, &db_path, Some(&home), &initial)
        .await
        .expect("open store");

    // Delete Videos on disk while keeping Pictures intact, then disable both slots.
    std::fs::remove_dir(&videos).expect("remove Videos directory");
    let disabled = XdgUserDirs::from_slots([
        (XdgUserDirSlot::Pictures, None),
        (XdgUserDirSlot::Videos, None),
    ]);
    let sync = store
        .sync_xdg_user_dirs(Some(&home), &disabled)
        .await
        .expect("sync disabled slots");
    assert!(sync.changed);
    assert_eq!(
        sync.places.quick_access,
        vec![Address::from(pictures.as_path())]
    );
}

#[tokio::test]
async fn legacy_db_without_xdg_snapshot_migrates_missing_fallback_and_tracks_future_edits() {
    use user_dirs::XdgUserDirSlot;

    let root = tempfile::tempdir().expect("create fixture directory");
    let home = root.path().join("home");
    let db_path = root.path().join("legacy.sqlite");
    let desktop = home.join("Desktop");
    let missing_downloads = home.join("Downloads");
    let zh_downloads = home.join("下载");
    let incoming = home.join("Incoming");
    std::fs::create_dir_all(&desktop).expect("create Desktop");
    std::fs::create_dir_all(&zh_downloads).expect("create zh downloads");
    std::fs::create_dir_all(&incoming).expect("create Incoming");

    let scope =
        gds::service::task_manager::Scope::with_handle(tokio::runtime::Handle::current(), false);
    // Simulate a legacy DB that recorded quick_access_seeded=1 without xdg_user_dirs_snapshot.
    let legacy = Store::open_at_with_quick_access(
        &scope,
        &db_path,
        &[
            Address::from(desktop.as_path()),
            Address::from(missing_downloads.as_path()),
        ],
    )
    .await
    .expect("open legacy store");
    drop(legacy);

    let upgraded_dirs = XdgUserDirs::from_slots([
        (XdgUserDirSlot::Desktop, Some(desktop.clone())),
        (XdgUserDirSlot::Downloads, Some(zh_downloads.clone())),
    ]);
    let store = Store::open_at_with_xdg_dirs(&scope, &db_path, Some(&home), &upgraded_dirs)
        .await
        .expect("upgrade legacy store with XDG dirs");
    assert_eq!(
        store.read().await.expect("read upgraded").quick_access,
        vec![
            Address::from(desktop.as_path()),
            Address::from(zh_downloads.as_path()),
        ]
    );

    // Subsequent relocations track from the newly saved snapshot.
    let next_dirs = XdgUserDirs::from_slots([
        (XdgUserDirSlot::Desktop, Some(desktop.clone())),
        (XdgUserDirSlot::Downloads, Some(incoming.clone())),
    ]);
    let sync = store
        .sync_xdg_user_dirs(Some(&home), &next_dirs)
        .await
        .expect("sync subsequent relocation");
    assert!(sync.changed);
    assert_eq!(
        sync.places.quick_access,
        vec![
            Address::from(desktop.as_path()),
            Address::from(incoming.as_path()),
        ]
    );
}
