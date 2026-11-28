use std::{
    ffi::{OsStr, OsString},
    fs,
    os::unix::{
        ffi::OsStringExt as _,
        fs::{MetadataExt as _, PermissionsExt as _},
    },
    path::{Path, PathBuf},
    time::Duration,
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{
    Action, Entry, Error, TrashWatch,
    directory_sizes::{self, DirectorySizesCache, SizeRecord},
    perform_cancellable, watch,
};
use crate::entities::file::directory::EntryKind;

struct TrashFixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    info_dir: PathBuf,
    files_dir: PathBuf,
    restore_root: PathBuf,
}

impl TrashFixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create trash test tempdir");
        let root = dir.path().join("Trash");
        let info_dir = root.join("info");
        let files_dir = root.join("files");
        let restore_root = dir.path().join("origin");
        fs::create_dir_all(&info_dir).expect("create trash info dir");
        fs::create_dir_all(&files_dir).expect("create trash files dir");
        fs::create_dir_all(&restore_root).expect("create original parent dir");
        Self {
            _dir: dir,
            root,
            info_dir,
            files_dir,
            restore_root,
        }
    }

    fn add_directory(&self, stem: &OsStr, original_name: &OsStr) -> (trash::TrashItem, i64) {
        let payload = self.files_dir.join(stem);
        fs::create_dir_all(&payload).expect("create trashed directory payload");
        fs::write(payload.join("inside.txt"), b"payload-bytes").expect("write child file");
        self.write_trashinfo(stem, original_name)
    }

    fn add_file(
        &self,
        stem: &OsStr,
        original_name: &OsStr,
        bytes: &[u8],
    ) -> (trash::TrashItem, i64) {
        let payload = self.files_dir.join(stem);
        fs::write(&payload, bytes).expect("write trashed file payload");
        self.write_trashinfo(stem, original_name)
    }

    fn write_trashinfo(&self, stem: &OsStr, original_name: &OsStr) -> (trash::TrashItem, i64) {
        let mut info_name = stem.to_os_string();
        info_name.push(".trashinfo");
        let info_path = self.info_dir.join(info_name);
        let original_path = self.restore_root.join(original_name);
        let content = format!(
            "[Trash Info]\nPath={}\nDeletionDate=2026-05-01T12:00:00\n",
            original_path.display()
        );
        fs::write(&info_path, content).expect("write .trashinfo");
        let mtime = fs::symlink_metadata(&info_path)
            .expect("stat .trashinfo")
            .mtime();
        let item = trash::TrashItem {
            id: info_path.into_os_string(),
            name: original_name.to_os_string(),
            original_parent: self.restore_root.clone(),
            time_deleted: 1_777_636_800,
        };
        (item, mtime)
    }

    fn write_directorysizes(&self, bytes: &[u8]) {
        fs::write(self.root.join("directorysizes"), bytes).expect("write directorysizes");
    }

    fn read_directorysizes(&self) -> Vec<u8> {
        fs::read(self.root.join("directorysizes")).expect("read directorysizes")
    }
}

#[test]
fn parses_directorysizes_with_percent_encoding_utf8_raw_bytes_and_skips_malformed_lines() {
    let mut raw = Vec::new();
    raw.extend_from_slice(b"1024 1700000000 plain\n");
    raw.extend_from_slice(b"2048 1700000001 my%20folder%20%281%29\n");
    raw.extend_from_slice(b"4096 1700000002 caf%C3%A9\r\n");
    raw.extend_from_slice(b"8192 1700000003 100%25\n");
    raw.extend_from_slice(b"512 1700000004 raw%FFbyte\n");
    raw.extend_from_slice(b"640 -5 literal\xFEbyte\n");
    // Duplicate entry: later record overrides earlier record.
    raw.extend_from_slice(b"111 1700000010 dup\n");
    raw.extend_from_slice(b"222 1700000020 dup\n");
    // Malformed lines that must be ignored.
    raw.extend_from_slice(b"\n");
    raw.extend_from_slice(b"# comment line\n");
    raw.extend_from_slice(b"notanumber 1700000000 bad1\n");
    raw.extend_from_slice(b"-10 1700000000 bad2\n");
    raw.extend_from_slice(b"4096 notmtime bad3\n");
    raw.extend_from_slice(b"4096 1700000000\n");
    raw.extend_from_slice(b"4096 1700000000 \n");
    raw.extend_from_slice(b"4096 1700000000 extra tokens\n");
    raw.extend_from_slice(b"4096 1700000000 bad%2Fslash\n");
    raw.extend_from_slice(b"4096 1700000000 ..\n");
    raw.extend_from_slice(b"4096 1700000000 bad%00nul\n");

    let parsed = directory_sizes::parse(&raw);
    assert_eq!(parsed.len(), 7);
    assert_eq!(
        parsed.get(OsStr::new("plain")),
        Some(&SizeRecord {
            size: 1024,
            mtime: 1_700_000_000,
        })
    );
    assert_eq!(
        parsed.get(OsStr::new("my folder (1)")),
        Some(&SizeRecord {
            size: 2048,
            mtime: 1_700_000_001,
        })
    );
    assert_eq!(
        parsed.get(OsStr::new("café")),
        Some(&SizeRecord {
            size: 4096,
            mtime: 1_700_000_002,
        })
    );
    assert_eq!(
        parsed.get(OsStr::new("100%")),
        Some(&SizeRecord {
            size: 8192,
            mtime: 1_700_000_003,
        })
    );
    assert_eq!(
        parsed.get(OsString::from_vec(b"raw\xffbyte".to_vec()).as_os_str()),
        Some(&SizeRecord {
            size: 512,
            mtime: 1_700_000_004,
        })
    );
    assert_eq!(
        parsed.get(OsString::from_vec(b"literal\xfebyte".to_vec()).as_os_str()),
        Some(&SizeRecord {
            size: 640,
            mtime: -5,
        })
    );
    assert_eq!(
        parsed.get(OsStr::new("dup")),
        Some(&SizeRecord {
            size: 222,
            mtime: 1_700_000_020,
        })
    );
}

#[test]
fn listing_populates_cached_directory_size_only_when_trashinfo_mtime_matches() {
    let fixture = TrashFixture::new();
    let (fresh_dir, fresh_mtime) =
        fixture.add_directory(OsStr::new("fresh dir"), OsStr::new("fresh dir"));
    let (stale_dir, stale_mtime) = fixture.add_directory(OsStr::new("stale"), OsStr::new("stale"));
    let (uncached_dir, _) = fixture.add_directory(OsStr::new("uncached"), OsStr::new("uncached"));
    let (regular_file, file_mtime) =
        fixture.add_file(OsStr::new("note.txt"), OsStr::new("note.txt"), b"hello!");

    let cache_text = format!(
        "987654 {fresh_mtime} fresh%20dir\n\
         555555 {} stale\n\
         999999 {file_mtime} note.txt\n",
        stale_mtime.saturating_sub(1)
    );
    fixture.write_directorysizes(cache_text.as_bytes());

    let mut cache = DirectorySizesCache::default();
    let entries = Entry::collect_with_sizes(
        vec![fresh_dir, stale_dir, uncached_dir, regular_file],
        &mut cache,
    )
    .expect("collect trash entries");

    assert_eq!(cache.loaded_root_count(), 1);
    assert_eq!(entries.len(), 4);

    assert_eq!(entries[0].kind, EntryKind::Directory);
    assert_eq!(entries[0].size, Some(987_654));

    assert_eq!(entries[1].kind, EntryKind::Directory);
    assert_eq!(entries[1].size, None);

    assert_eq!(entries[2].kind, EntryKind::Directory);
    assert_eq!(entries[2].size, None);

    assert_eq!(entries[3].kind, EntryKind::File);
    assert_eq!(entries[3].size, Some(6));
}

#[test]
fn restore_removes_directory_entry_atomically_and_preserves_other_lines_verbatim() {
    let fixture = TrashFixture::new();
    let (dir_item, mtime) = fixture.add_directory(OsStr::new("café dir"), OsStr::new("café dir"));

    // Lowercase hex `%c3%a9` must still match decoded stem `café dir`, while
    // unrelated lines (including comments, CRLF, and raw bytes) remain verbatim.
    let mut initial = Vec::new();
    initial.extend_from_slice(b"# foreign comment\n");
    initial.extend_from_slice(b"111 1700000001 keep%20first\r\n");
    initial.extend_from_slice(format!("42000 {mtime} caf%c3%a9%20dir\n").as_bytes());
    initial.extend_from_slice(b"222 1700000002 raw\xFEkeep\n");
    fixture.write_directorysizes(&initial);

    let mut cache = DirectorySizesCache::default();
    let entries =
        Entry::collect_with_sizes(vec![dir_item], &mut cache).expect("collect trashed dir");
    assert_eq!(entries[0].size, Some(42_000));

    let outcomes = perform_cancellable(Action::Restore, entries, &CancellationToken::new());
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].result.is_ok());
    assert!(fixture.restore_root.join("café dir/inside.txt").is_file());

    let mut expected = Vec::new();
    expected.extend_from_slice(b"# foreign comment\n");
    expected.extend_from_slice(b"111 1700000001 keep%20first\r\n");
    expected.extend_from_slice(b"222 1700000002 raw\xFEkeep\n");
    assert_eq!(fixture.read_directorysizes(), expected);

    // Ensure no temporary `.directorysizes.*` files were left behind.
    let leftover_temps: Vec<_> = fs::read_dir(&fixture.root)
        .expect("read trash root")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".directorysizes.")
        })
        .collect();
    assert!(leftover_temps.is_empty());
}

#[test]
fn purge_removes_directory_entry_atomically_and_preserves_other_lines_verbatim() {
    let fixture = TrashFixture::new();
    let (dir_item, mtime) = fixture.add_directory(OsStr::new("old build"), OsStr::new("old build"));
    let initial = format!(
        "100 1700000001 alpha%20dir\n\
         9000 {mtime} old%20build\n\
         300 1700000002 omega%20dir\n"
    );
    fixture.write_directorysizes(initial.as_bytes());

    let mut cache = DirectorySizesCache::default();
    let entries =
        Entry::collect_with_sizes(vec![dir_item], &mut cache).expect("collect trashed dir");
    assert_eq!(entries[0].size, Some(9000));

    let outcomes = perform_cancellable(Action::Purge, entries, &CancellationToken::new());
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].result.is_ok());
    assert!(!fixture.files_dir.join("old build").exists());
    assert!(!fixture.info_dir.join("old build.trashinfo").exists());

    assert_eq!(
        fixture.read_directorysizes(),
        b"100 1700000001 alpha%20dir\n300 1700000002 omega%20dir\n"
    );
}

#[test]
fn failed_restore_keeps_directorysizes_entry_and_missing_cache_is_non_fatal() {
    let fixture = TrashFixture::new();
    let (conflict_item, conflict_mtime) =
        fixture.add_directory(OsStr::new("blocked"), OsStr::new("blocked"));
    fs::create_dir_all(fixture.restore_root.join("blocked")).expect("create conflicting target");
    let initial = format!("777 {conflict_mtime} blocked\n");
    fixture.write_directorysizes(initial.as_bytes());

    let mut cache = DirectorySizesCache::default();
    let entries =
        Entry::collect_with_sizes(vec![conflict_item], &mut cache).expect("collect blocked dir");
    let outcomes = perform_cancellable(Action::Restore, entries, &CancellationToken::new());
    assert!(matches!(
        outcomes[0].result,
        Err(Error::RestoreConflict { .. })
    ));
    assert_eq!(fixture.read_directorysizes(), initial.as_bytes());

    // Purge succeeds cleanly when directorysizes does not exist at all.
    fs::remove_file(fixture.root.join("directorysizes")).expect("remove cache file");
    let (no_cache_item, _) = fixture.add_directory(OsStr::new("plain"), OsStr::new("plain"));
    let mut cache = DirectorySizesCache::default();
    let entries =
        Entry::collect_with_sizes(vec![no_cache_item], &mut cache).expect("collect plain dir");
    let outcomes = perform_cancellable(Action::Purge, entries, &CancellationToken::new());
    assert!(outcomes[0].result.is_ok());
    assert!(!fixture.root.join("directorysizes").exists());
}

#[test]
fn resolve_home_trash_info_ignores_relative_xdg_data_home_and_falls_back_to_home() {
    assert_eq!(
        watch::resolve_home_trash_info(
            Some(OsStr::new("/srv/xdg-data")),
            Some(OsStr::new("/home/alice")),
        ),
        Some(PathBuf::from("/srv/xdg-data/Trash/info"))
    );
    assert_eq!(
        watch::resolve_home_trash_info(
            Some(OsStr::new("relative/data")),
            Some(OsStr::new("/home/alice")),
        ),
        Some(PathBuf::from("/home/alice/.local/share/Trash/info"))
    );
    assert_eq!(
        watch::resolve_home_trash_info(Some(OsStr::new("")), Some(OsStr::new("/home/alice"))),
        Some(PathBuf::from("/home/alice/.local/share/Trash/info"))
    );
    assert_eq!(
        watch::resolve_home_trash_info(None, Some(OsStr::new("/home/alice"))),
        Some(PathBuf::from("/home/alice/.local/share/Trash/info"))
    );
    assert_eq!(
        watch::resolve_home_trash_info(Some(OsStr::new("rel")), Some(OsStr::new("rel-home"))),
        None
    );
}

#[test]
fn mountinfo_discovery_finds_valid_local_mount_trashes_and_rejects_invalid_or_remote() {
    let base = tempfile::tempdir().expect("create mount fixture root");
    let uid = 1000_u32;

    // 1. Valid sticky `.Trash/$uid/info`
    let shared_mount = base.path().join("shared disk");
    let shared_dot_trash = shared_mount.join(".Trash");
    let shared_info = shared_dot_trash.join("1000/info");
    fs::create_dir_all(&shared_info).expect("create shared trash info");
    fs::set_permissions(&shared_dot_trash, fs::Permissions::from_mode(0o1777))
        .expect("set sticky bit on .Trash");

    // 2. Valid `.Trash-$uid/info`
    let user_mount = base.path().join("usb");
    let user_info = user_mount.join(".Trash-1000/info");
    fs::create_dir_all(&user_info).expect("create user mount trash info");

    // 3. Invalid non-sticky `.Trash/$uid/info`
    let non_sticky_mount = base.path().join("nonsticky");
    let non_sticky_dot = non_sticky_mount.join(".Trash");
    let non_sticky_info = non_sticky_dot.join("1000/info");
    fs::create_dir_all(&non_sticky_info).expect("create non-sticky trash info");
    fs::set_permissions(&non_sticky_dot, fs::Permissions::from_mode(0o755))
        .expect("clear sticky bit on .Trash");

    // 4. Invalid symlink `.Trash`
    let symlink_mount = base.path().join("symmount");
    fs::create_dir_all(&symlink_mount).expect("create symlink mount");
    std::os::unix::fs::symlink(&shared_dot_trash, symlink_mount.join(".Trash"))
        .expect("create symlink .Trash");

    // 5. Remote NFS mount (must be skipped without probing)
    let nfs_mount = base.path().join("nfs");
    fs::create_dir_all(nfs_mount.join(".Trash-1000/info")).expect("create nfs trash info");

    let shared_escaped = shared_mount.to_string_lossy().replace(' ', r"\040");
    let mountinfo = format!(
        "20 1 8:1 / {shared_escaped} rw,relatime - ext4 /dev/sda1 rw\n\
         21 1 8:2 / {} rw,relatime - exfat /dev/sdb1 rw\n\
         22 1 8:3 / {} rw,relatime - ext4 /dev/sdc1 rw\n\
         23 1 8:4 / {} rw,relatime - ext4 /dev/sdd1 rw\n\
         24 1 0:50 / {} rw,relatime - nfs4 server:/export rw\n",
        user_mount.display(),
        non_sticky_mount.display(),
        symlink_mount.display(),
        nfs_mount.display(),
    );

    let found = watch::mount_trash_info_dirs_from_mountinfo(mountinfo.as_bytes(), uid);
    assert_eq!(found.len(), 2);
    assert!(found.contains(&shared_info));
    assert!(found.contains(&user_info));
}

async fn expect_watch_event(receiver: &mut mpsc::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(3), receiver.recv())
        .await
        .expect("expected trash watch notification within timeout")
        .expect("trash watch channel stayed open");
}

async fn drain_quiet(receiver: &mut mpsc::Receiver<()>) {
    while let Ok(item) = tokio::time::timeout(Duration::from_millis(150), receiver.recv()).await {
        item.expect("trash watch channel stayed open");
    }
}

#[tokio::test]
async fn trash_watch_observes_existing_info_changes_directorysizes_and_ignores_unrelated() {
    let dir = tempfile::tempdir().expect("create watch tempdir");
    let trash_root = dir.path().join("Trash");
    let info_dir = trash_root.join("info");
    fs::create_dir_all(&info_dir).expect("create trash info dir");

    let (tx, mut rx) = mpsc::channel(4);
    let mut watch = TrashWatch::new(tx);
    watch
        .set_directories(vec![info_dir.clone()])
        .expect("install trash watch");

    // Creating a `.trashinfo` file triggers an event.
    let record = info_dir.join("doc.txt.trashinfo");
    fs::write(&record, "[Trash Info]\nPath=/tmp/doc.txt\n").expect("write trashinfo");
    expect_watch_event(&mut rx).await;
    drain_quiet(&mut rx).await;

    // Removing the `.trashinfo` file triggers an event.
    fs::remove_file(&record).expect("remove trashinfo");
    expect_watch_event(&mut rx).await;
    drain_quiet(&mut rx).await;

    // Atomically updating `$trash/directorysizes` triggers an event.
    let tmp = trash_root.join(".directorysizes.tmp");
    fs::write(&tmp, "1024 1700000000 folder\n").expect("write temp directorysizes");
    fs::rename(&tmp, trash_root.join("directorysizes")).expect("rename directorysizes");
    expect_watch_event(&mut rx).await;
    drain_quiet(&mut rx).await;

    // Unrelated sibling file outside `Trash/` is ignored.
    fs::write(dir.path().join("recently-used.xbel"), "unrelated").expect("write unrelated file");
    assert!(
        tokio::time::timeout(Duration::from_millis(150), rx.recv())
            .await
            .is_err(),
        "unrelated sibling write must not trigger trash watch"
    );
}

#[tokio::test]
async fn trash_watch_follows_missing_trash_info_created_later() {
    let dir = tempfile::tempdir().expect("create missing-trash tempdir");
    let info_dir = dir.path().join("Trash/info");
    assert!(!Path::new(&info_dir).exists());

    let (tx, mut rx) = mpsc::channel(4);
    let mut watch = TrashWatch::new(tx);
    watch
        .set_directories(vec![info_dir.clone()])
        .expect("install watch on missing trash info dir");

    // Unrelated file in existing ancestor must be ignored.
    fs::write(dir.path().join("unrelated.txt"), "ignore").expect("write unrelated");
    assert!(
        tokio::time::timeout(Duration::from_millis(150), rx.recv())
            .await
            .is_err(),
        "unrelated file in ancestor must not trigger trash watch"
    );

    // Creating `Trash/info` fires via ancestor watch; `refresh()` binds directly to `info/`.
    fs::create_dir_all(&info_dir).expect("create missing Trash/info");
    expect_watch_event(&mut rx).await;
    watch.refresh().expect("rebind watch to created Trash/info");
    drain_quiet(&mut rx).await;

    // Subsequent `.trashinfo` creation inside `info/` is observed directly.
    fs::write(
        info_dir.join("later.trashinfo"),
        "[Trash Info]\nPath=/tmp/later\n",
    )
    .expect("write trashinfo after creation");
    expect_watch_event(&mut rx).await;
}
