use super::{Direction, Entry, History, Kind, Step, transfer_steps};
use crate::features::file_operations::{new_folder, rename, transfer};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::MetadataExt,
};
use tokio_util::sync::CancellationToken;

#[test]
fn move_undo_preserves_in_place_edits_across_undo_and_redo() {
    let root = tempfile::tempdir().expect("isolated history tempdir");
    let original = root.path().join("draft.txt");
    fs::write(&original, b"v1").expect("write original file");

    let renamed = rename::Request {
        source: original.clone(),
        name: "final.txt".into(),
    }
    .execute()
    .expect("execute rename");

    let mut history = History::default();
    history.record(Entry::new(
        Kind::Rename,
        vec![Step::Move {
            source: renamed.destination.clone(),
            destination: original.clone(),
            expected: renamed.snapshot.expect("captured rename snapshot"),
        }],
    ));

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&renamed.destination)
        .expect("open renamed file in place");
    file.write_all(b"edited in place").expect("edit in place");
    drop(file);

    let cancel = CancellationToken::new();
    let mut undo_report = history
        .take(Direction::Undo)
        .expect("undo entry")
        .execute_cancellable(&cancel);
    assert!(undo_report.errors.is_empty());
    assert_eq!(
        fs::read(&original).expect("restored original"),
        b"edited in place"
    );
    assert!(!renamed.destination.exists());
    history.finish(Direction::Undo, &mut undo_report);

    let mut redo_report = history
        .take(Direction::Redo)
        .expect("redo entry")
        .execute_cancellable(&cancel);
    assert!(redo_report.errors.is_empty());
    assert_eq!(
        fs::read(&renamed.destination).expect("re-applied rename"),
        b"edited in place"
    );
    assert!(!original.exists());
    history.finish(Direction::Redo, &mut redo_report);
}

#[test]
fn move_undo_rejects_replaced_inode_at_same_path() {
    let root = tempfile::tempdir().expect("isolated history tempdir");
    let source_dir = root.path().join("from");
    let dest_dir = root.path().join("to");
    fs::create_dir(&source_dir).expect("create source dir");
    fs::create_dir(&dest_dir).expect("create dest dir");
    let source = source_dir.join("item.txt");
    fs::write(&source, b"moved").expect("write source");

    let report = transfer::Batch::new(transfer::Request {
        paths: vec![source.clone()],
        destination: dest_dir.clone(),
        mode: transfer::Mode::Move,
    })
    .expect("move batch")
    .execute();
    let steps = transfer_steps(&report);
    assert_eq!(steps.len(), 1);

    let destination = dest_dir.join("item.txt");
    let replacement = root.path().join("replacement.txt");
    fs::write(&replacement, b"unrelated").expect("write replacement");
    fs::rename(&replacement, &destination).expect("replace destination inode");

    let undo = Entry::new(Kind::Move, steps).execute_cancellable(&CancellationToken::new());
    assert_eq!(undo.errors.len(), 1);
    assert_eq!(undo.remaining.len(), 1);
    assert!(!source.exists());
    assert_eq!(
        fs::read(&destination).expect("replacement untouched"),
        b"unrelated"
    );
}

#[test]
fn copy_undo_rejects_in_place_edits_and_same_timestamp_inode_replacements() {
    use rustix::fs::{AtFlags, CWD, Timespec, Timestamps, UTIME_OMIT, utimensat};

    let root = tempfile::tempdir().expect("isolated history tempdir");
    let source = root.path().join("origin.txt");
    let first_dir = root.path().join("copy-edited");
    let second_dir = root.path().join("copy-replaced");
    fs::write(&source, b"copied").expect("write source");
    fs::create_dir(&first_dir).expect("create first copy dir");
    fs::create_dir(&second_dir).expect("create second copy dir");

    let first_report = transfer::Batch::new(transfer::Request {
        paths: vec![source.clone()],
        destination: first_dir.clone(),
        mode: transfer::Mode::Copy,
    })
    .expect("first copy batch")
    .execute();
    let edited_copy = first_dir.join("origin.txt");
    let mut handle = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&edited_copy)
        .expect("open copied file in place");
    handle.write_all(b"user work").expect("write user work");
    drop(handle);

    let first_undo = Entry::new(Kind::Copy, transfer_steps(&first_report))
        .execute_cancellable(&CancellationToken::new());
    assert_eq!(first_undo.errors.len(), 1);
    assert_eq!(
        fs::read(&edited_copy).expect("edited copy survives"),
        b"user work"
    );

    let second_report = transfer::Batch::new(transfer::Request {
        paths: vec![source],
        destination: second_dir.clone(),
        mode: transfer::Mode::Copy,
    })
    .expect("second copy batch")
    .execute();
    let replaced_copy = second_dir.join("origin.txt");
    let meta = fs::symlink_metadata(&replaced_copy).expect("copied metadata");
    let replacement = root.path().join("same-size.txt");
    fs::write(&replacement, b"secret").expect("write same-size replacement");
    utimensat(
        CWD,
        &replacement,
        &Timestamps {
            last_access: Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_OMIT,
            },
            last_modification: Timespec {
                tv_sec: meta.mtime() as _,
                tv_nsec: meta.mtime_nsec() as _,
            },
        },
        AtFlags::SYMLINK_NOFOLLOW,
    )
    .expect("match mtime on replacement inode");
    fs::rename(&replacement, &replaced_copy).expect("swap inode with identical size and mtime");

    let second_undo = Entry::new(Kind::Copy, transfer_steps(&second_report))
        .execute_cancellable(&CancellationToken::new());
    assert_eq!(second_undo.errors.len(), 1);
    assert_eq!(
        fs::read(&replaced_copy).expect("replaced copy survives"),
        b"secret"
    );
}

#[test]
fn new_folder_undo_rejects_non_empty_or_replaced_directories() {
    let root = tempfile::tempdir().expect("isolated history tempdir");
    let created = new_folder::Request {
        directory: root.path().to_path_buf(),
        name: "Project".into(),
    }
    .execute()
    .expect("create folder");
    let step = Step::trash_empty_directory(
        created.path.clone(),
        created.snapshot.expect("captured folder snapshot"),
    );

    let child = created.path.join("notes.md");
    fs::write(&child, b"draft").expect("populate folder");
    let occupied_undo = Entry::new(Kind::NewFolder, vec![step.clone()])
        .execute_cancellable(&CancellationToken::new());
    assert_eq!(occupied_undo.errors.len(), 1);
    assert_eq!(occupied_undo.remaining.len(), 1);
    assert_eq!(fs::read(&child).expect("child survives"), b"draft");

    fs::remove_dir_all(&created.path).expect("remove original folder");
    fs::create_dir(&created.path).expect("recreate empty folder with new inode");
    let replaced_undo =
        Entry::new(Kind::NewFolder, vec![step]).execute_cancellable(&CancellationToken::new());
    assert_eq!(replaced_undo.errors.len(), 1);
    assert!(created.path.is_dir());
}
