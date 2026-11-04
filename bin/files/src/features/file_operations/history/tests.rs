use super::*;
use std::fs;

fn replay(history: &mut History, direction: Direction) -> Report {
    let mut report = history.take(direction).expect("history entry").execute();
    history.finish(direction, &mut report);
    report
}

#[test]
fn moves_retry_only_failed_items_and_new_operations_discard_redo() {
    let root = tempfile::tempdir().expect("history fixture");
    let original = root.path().join("original");
    let target = root.path().join("target");
    fs::create_dir(&original).unwrap();
    fs::create_dir(&target).unwrap();
    for name in ["one", "two"] {
        fs::write(original.join(name), name).unwrap();
    }
    let result = transfer::Batch::new(transfer::Request {
        paths: vec![original.join("one"), original.join("two")],
        destination: target.clone(),
        mode: transfer::Mode::Move,
    })
    .unwrap()
    .execute();
    let mut history = History::default();
    history.record(Entry::new(Kind::Move, transfer_steps(&result)));
    fs::write(original.join("one"), "collision").unwrap();
    let report = replay(&mut history, Direction::Undo);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(
        fs::read_to_string(original.join("one")).unwrap(),
        "collision"
    );
    assert_eq!(fs::read_to_string(original.join("two")).unwrap(), "two");
    assert!(target.join("one").exists());
    assert!(!target.join("two").exists());
    fs::remove_file(original.join("one")).unwrap();
    assert!(replay(&mut history, Direction::Undo).errors.is_empty());
    assert!(history.peek(Direction::Undo).is_none());
    for _ in 0..2 {
        assert!(replay(&mut history, Direction::Redo).errors.is_empty());
    }
    assert!(target.join("one").exists() && target.join("two").exists());
    assert!(replay(&mut history, Direction::Undo).errors.is_empty());
    history.record(Entry::new(
        Kind::NewFolder,
        vec![Step::trash(root.path().join("new"))],
    ));
    assert!(history.peek(Direction::Redo).is_none());
}

/// Run with a private home/trash in a subprocess: changing XDG variables in
/// the parallel test process would affect unrelated filesystem operations.
#[test]
fn copy_create_trash_and_restore_round_trip() {
    const CHILD: &str = "GDS_HISTORY_FIXTURE";
    let Some(root) = std::env::var_os(CHILD) else {
        let root = tempfile::tempdir().expect("private trash fixture");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "features::file_operations::history::tests::copy_create_trash_and_restore_round_trip", "--nocapture"])
            .env(CHILD, root.path()).env("HOME", root.path())
            .env("XDG_DATA_HOME", root.path().join("data"))
            .output().expect("run isolated trash test");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    };
    let root = PathBuf::from(root);
    fs::create_dir(root.join("data")).unwrap();
    let original = root.join("original");
    let target = root.join("target");
    fs::create_dir(&original).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(original.join("file"), "original contents").unwrap();
    let report = transfer::Batch::new(transfer::Request {
        paths: vec![original.join("file")],
        destination: target.clone(),
        mode: transfer::Mode::Copy,
    })
    .unwrap()
    .execute();
    let mut history = History::default();
    history.record(Entry::new(Kind::Copy, transfer_steps(&report)));
    // Preserve edits to the copy across undo and redo, independent of its source.
    fs::write(target.join("file"), "edited copy").unwrap();
    assert!(replay(&mut history, Direction::Undo).errors.is_empty());
    assert!(!target.join("file").exists());
    assert!(replay(&mut history, Direction::Redo).errors.is_empty());
    assert_eq!(
        fs::read_to_string(target.join("file")).unwrap(),
        "edited copy"
    );

    let folder = root.join("new folder");
    fs::create_dir(&folder).unwrap();
    history.record(Entry::new(
        Kind::NewFolder,
        vec![Step::trash(folder.clone())],
    ));
    for _ in 0..2 {
        assert!(replay(&mut history, Direction::Undo).errors.is_empty());
        assert!(!folder.exists());
        assert!(replay(&mut history, Direction::Redo).errors.is_empty());
        assert!(folder.is_dir());
    }
    // Older records with the same path must never be mistaken for this deletion.
    let older = recycle::trash(&target.join("file")).unwrap().entry.unwrap();
    fs::write(target.join("file"), "newer file").unwrap();
    let receipt = recycle::trash(&target.join("file")).unwrap().entry.unwrap();
    assert_ne!(older.id, receipt.id);
    history.record(Entry::new(Kind::Trash, vec![Step::Restore(receipt)]));
    assert!(replay(&mut history, Direction::Undo).errors.is_empty());
    assert_eq!(
        fs::read_to_string(target.join("file")).unwrap(),
        "newer file"
    );
    assert!(replay(&mut history, Direction::Redo).errors.is_empty());
    assert!(replay(&mut history, Direction::Undo).errors.is_empty());
    // A separate Restore action must preserve the preceding Trash operation
    // through multiple undo/redo cycles, despite the regenerated trash id.
    let receipt = recycle::trash(&target.join("file")).unwrap().entry.unwrap();
    history.record(Entry::new(
        Kind::Trash,
        vec![Step::Restore(receipt.clone())],
    ));
    recycle::perform(recycle::Action::Restore, vec![receipt.clone()])
        .pop()
        .unwrap()
        .result
        .unwrap();
    history.record(Entry::new(
        Kind::Restore,
        vec![Step::undo_restore(&receipt)],
    ));
    for _ in 0..2 {
        assert!(replay(&mut history, Direction::Undo).errors.is_empty());
        assert!(!target.join("file").exists());
        assert!(replay(&mut history, Direction::Undo).errors.is_empty());
        assert_eq!(
            fs::read_to_string(target.join("file")).unwrap(),
            "newer file"
        );
        assert!(replay(&mut history, Direction::Redo).errors.is_empty());
        assert!(!target.join("file").exists());
        assert!(replay(&mut history, Direction::Redo).errors.is_empty());
        assert_eq!(
            fs::read_to_string(target.join("file")).unwrap(),
            "newer file"
        );
    }
    for outcome in recycle::perform(recycle::Action::Purge, vec![older]) {
        outcome.result.unwrap();
    }
}

#[test]
fn cancelled_history_retains_steps_for_retry() {
    let root = tempfile::tempdir().expect("history fixture");
    let path = root.path().join("keep");
    fs::write(&path, "keep").unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    let report =
        Entry::new(Kind::Copy, vec![Step::trash(path.clone())]).execute_cancellable(&cancel);
    assert!(path.exists());
    assert_eq!(report.remaining.len(), 1);
    assert!(report.inverse.is_empty());
}
