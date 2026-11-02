use std::{
    ffi::{OsStr, OsString},
    fs,
    os::unix::{ffi::OsStringExt, fs::symlink},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use super::{Action, Entry, Error, collect_entries, perform};
use crate::entities::file::directory::EntryKind;

/// A self-contained freedesktop trash store; tests never touch the real trash.
struct Store {
    root: tempfile::TempDir,
}

impl Store {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("create trash fixture");
        fs::create_dir_all(root.path().join("Trash/info")).expect("create trash info folder");
        fs::create_dir_all(root.path().join("Trash/files")).expect("create trash files folder");
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn info_dir(&self) -> PathBuf {
        self.path().join("Trash/info")
    }

    fn files_dir(&self) -> PathBuf {
        self.path().join("Trash/files")
    }

    fn record(
        &self,
        payload_name: &OsStr,
        original_name: OsString,
        original_parent: PathBuf,
        contents: &[u8],
    ) -> Entry {
        fs::write(self.files_dir().join(payload_name), contents).expect("write payload");
        self.register(payload_name, original_name, original_parent)
    }

    /// Register an already-created payload and return its listed entry.
    fn register(
        &self,
        payload_name: &OsStr,
        original_name: OsString,
        original_parent: PathBuf,
    ) -> Entry {
        Entry::from_item(self.item(payload_name, original_name, original_parent))
            .expect("fixture is a valid trash record")
    }

    /// Write an info record and return the raw item, leaving the payload to the
    /// caller so a fixture can describe one that was never written.
    fn item(
        &self,
        payload_name: &OsStr,
        original_name: OsString,
        original_parent: PathBuf,
    ) -> trash::TrashItem {
        let mut info_name = payload_name.to_owned();
        info_name.push(".trashinfo");
        let info = self.info_dir().join(info_name);
        fs::write(&info, b"[Trash Info]\n").expect("write info record");
        trash::TrashItem {
            id: info.into_os_string(),
            name: original_name,
            original_parent,
            time_deleted: 0,
        }
    }
}

#[test]
fn lists_readable_records_and_skips_corrupt_ones() {
    let store = Store::new();
    let parent = store.path().join("documents");
    fs::write(store.files_dir().join("note.txt"), b"hello").expect("write payload");
    let readable = store.item(
        OsStr::new("note.txt"),
        OsString::from("note.txt"),
        parent.clone(),
    );
    let readable_id = readable.id.clone();
    // A record whose payload is missing cannot be acted on.
    let corrupt = store.item(OsStr::new("gone.txt"), OsString::from("gone.txt"), parent);

    let entries = collect_entries([corrupt, readable]).expect("the readable record survives");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, readable_id);
    assert!(
        collect_entries(Vec::new())
            .expect("an empty trash is not a failure")
            .is_empty()
    );
}

#[test]
fn reports_an_error_when_no_record_can_be_read() {
    let store = Store::new();
    let parent = store.path().join("documents");
    let items = [
        store.item(OsStr::new("a"), OsString::from("a"), parent.clone()),
        store.item(OsStr::new("b"), OsString::from("b"), parent),
    ];

    match collect_entries(items) {
        Err(Error::Unreadable { failed, .. }) => assert_eq!(failed, 2),
        other => panic!("expected an unreadable-trash error, got {other:?}"),
    }
}

#[test]
fn restores_payload_and_removes_its_record() {
    let store = Store::new();
    let parent = store.path().join("documents");
    let entry = store.record(
        OsStr::new("note.txt"),
        OsString::from("note.txt"),
        parent.clone(),
        b"hello",
    );
    let id = entry.id.clone();
    assert_eq!(entry.kind, EntryKind::File);
    assert_eq!(entry.size, Some(5));
    assert_eq!(entry.deleted_at, Some(UNIX_EPOCH));

    let outcomes = perform(Action::Restore, vec![entry]);

    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].original_path, parent.join("note.txt"));
    assert!(
        outcomes[0].result.is_ok(),
        "restore failed: {:?}",
        outcomes[0].result
    );
    assert_eq!(
        fs::read(parent.join("note.txt")).expect("read restored file"),
        b"hello"
    );
    assert!(!Path::new(&id).exists());
    assert!(!store.files_dir().join("note.txt").exists());
}

#[test]
fn refuses_to_overwrite_an_occupied_original_location() {
    let store = Store::new();
    let parent = store.path().join("documents");
    fs::create_dir_all(&parent).expect("create original parent");
    fs::write(parent.join("note.txt"), b"keep").expect("write existing file");
    let entry = store.record(
        OsStr::new("note.txt"),
        OsString::from("note.txt"),
        parent.clone(),
        b"trashed",
    );
    let id = entry.id.clone();

    let outcomes = perform(Action::Restore, vec![entry]);

    match &outcomes[0].result {
        Err(Error::RestoreConflict { path }) => {
            assert!(path.ends_with("documents/note.txt"));
        }
        other => panic!("expected a restore conflict, got {other:?}"),
    }
    assert_eq!(
        fs::read(parent.join("note.txt")).expect("read untouched file"),
        b"keep"
    );
    assert_eq!(
        fs::read(store.files_dir().join("note.txt")).expect("payload stays trashed"),
        b"trashed"
    );
    assert!(Path::new(&id).exists());
}

#[test]
fn purging_a_link_never_removes_its_target() {
    let store = Store::new();
    let target = store.path().join("outside.txt");
    fs::write(&target, b"safe").expect("write link target");
    let link = store.files_dir().join("link");
    symlink(&target, &link).expect("create payload link");
    let entry = store.register(
        OsStr::new("link"),
        OsString::from("link"),
        store.path().join("elsewhere"),
    );
    assert_eq!(entry.kind, EntryKind::SymbolicLink);

    let outcomes = perform(Action::Purge, vec![entry]);

    assert!(
        outcomes[0].result.is_ok(),
        "purge failed: {:?}",
        outcomes[0].result
    );
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(fs::read(&target).expect("read link target"), b"safe");
    assert!(!store.info_dir().join("link.trashinfo").exists());
}

#[test]
fn distinguishes_records_that_share_an_original_name() {
    let store = Store::new();
    let first_parent = store.path().join("first");
    let second_parent = store.path().join("second");
    let first = store.record(
        OsStr::new("a"),
        OsString::from("same.txt"),
        first_parent.clone(),
        b"one",
    );
    let second = store.record(
        OsStr::new("b"),
        OsString::from("same.txt"),
        second_parent.clone(),
        b"two",
    );
    assert_ne!(first.id, second.id);
    assert_eq!(first.name, second.name);

    let outcomes = perform(Action::Restore, vec![first, second]);

    assert!(outcomes.iter().all(|outcome| outcome.result.is_ok()));
    assert_eq!(
        fs::read(first_parent.join("same.txt")).expect("read first restored file"),
        b"one"
    );
    assert_eq!(
        fs::read(second_parent.join("same.txt")).expect("read second restored file"),
        b"two"
    );
    assert!(!store.files_dir().join("a").exists());
    assert!(!store.files_dir().join("b").exists());
}

#[test]
fn refuses_a_record_whose_payload_was_replaced() {
    let store = Store::new();
    let parent = store.path().join("documents");
    let entry = store.record(
        OsStr::new("note.txt"),
        OsString::from("note.txt"),
        parent.clone(),
        b"hello",
    );
    let payload = store.files_dir().join("note.txt");
    fs::remove_file(&payload).expect("remove listed payload");
    fs::write(&payload, b"replacement").expect("replace payload");

    let outcomes = perform(Action::Restore, vec![entry]);

    assert!(matches!(&outcomes[0].result, Err(Error::Stale { .. })));
    assert!(!parent.exists());
    assert_eq!(
        fs::read(&payload).expect("replacement is untouched"),
        b"replacement"
    );
    assert!(store.info_dir().join("note.txt.trashinfo").exists());
}

#[test]
fn restores_non_utf8_names_without_rebuilding_them() {
    let store = Store::new();
    let name = OsString::from_vec(b"caf\xe9".to_vec());
    let parent = store.path().join("documents");
    let entry = store.record(&name, name.clone(), parent.clone(), b"data");
    assert_eq!(entry.name, name);
    assert_eq!(entry.original_path, parent.join(&name));
    let id = entry.id.clone();

    let outcomes = perform(Action::Restore, vec![entry]);

    assert!(
        outcomes[0].result.is_ok(),
        "restore failed: {:?}",
        outcomes[0].result
    );
    assert_eq!(
        fs::read(parent.join(&name)).expect("read restored non-UTF-8 file"),
        b"data"
    );
    assert!(!Path::new(&id).exists());
    assert!(!store.files_dir().join(&name).exists());
}

#[test]
fn reads_real_trashinfo_and_restores_without_overwriting() {
    const FIXTURE: &str = "GDS_TRASH_FIXTURE";
    if std::env::var_os(FIXTURE).is_none() {
        let root = tempfile::tempdir().expect("isolated XDG data");
        let result = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "pages::workspace::ui::recycle::backend::tests::reads_real_trashinfo_and_restores_without_overwriting", "--nocapture"])
            .env(FIXTURE, root.path())
            .env("XDG_DATA_HOME", root.path())
            .output().expect("run isolated trash workflow");
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
        return;
    }
    let root = PathBuf::from(std::env::var_os(FIXTURE).expect("fixture"));
    let info = root.join("Trash/info");
    let files = root.join("Trash/files");
    fs::create_dir_all(&info).expect("info");
    fs::create_dir_all(&files).expect("files");
    for (id, record) in [
        (
            "relative",
            "# comment\nPath=documents/a=b%FF.txt\nPath=/wrong/place\nDeletionDate=2026-01-01T12:00:00\nDeletionDate=invalid\nextension without equals\n",
        ),
        ("conflict", "Path=documents/existing\n"),
        ("escape", "Path=%2e%2e/escape\n"),
        ("missing", "unknown extension\n"),
    ] {
        fs::write(
            info.join(format!("{id}.trashinfo")),
            format!("[Trash Info]\n\n{record}"),
        )
        .expect("record");
        fs::write(files.join(id), b"trashed").expect("payload");
    }
    fs::create_dir_all(root.join("documents")).expect("original directory");
    fs::write(root.join("documents/existing"), b"keep").expect("existing target");
    let entries: Vec<_> = super::list()
        .expect("enumerate real records")
        .into_iter()
        .filter(|entry| Path::new(&entry.id).starts_with(&info))
        .collect();
    assert_eq!(entries.len(), 2, "invalid records are skipped");
    let relative = entries
        .iter()
        .find(|entry| entry.id == info.join("relative.trashinfo").as_os_str())
        .expect("relative record");
    let expected = root
        .join("documents")
        .join(OsString::from_vec(b"a=b\xff.txt".to_vec()));
    assert_eq!(relative.original_path, expected);
    assert!(relative.deleted_at.is_some(), "first deletion date wins");
    let outcomes = perform(Action::Restore, entries);
    assert!(
        outcomes
            .iter()
            .find(|outcome| outcome.original_path == expected)
            .expect("restored result")
            .result
            .is_ok()
    );
    assert_eq!(
        fs::read(expected).expect("restored non-UTF-8 file"),
        b"trashed"
    );
    assert!(matches!(
        outcomes
            .iter()
            .find(|outcome| outcome.original_path.ends_with("existing"))
            .expect("conflict result")
            .result,
        Err(Error::RestoreConflict { .. })
    ));
    assert_eq!(
        fs::read(root.join("documents/existing")).expect("untouched target"),
        b"keep"
    );
    assert!(files.join("conflict").exists());
}
