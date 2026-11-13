use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};
use tokio_util::sync::CancellationToken;

#[test]
fn save_and_read_refuse_symlinks_without_modifying_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    fs::write(&target, b"hello").expect("write target");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).expect("set target mode");
    symlink(&target, &link).expect("create symlink");

    assert!(matches!(read(&link), Err(Error::Symlink)));
    assert!(matches!(save(&link, 0o777), Err(Error::Symlink)));
    assert_eq!(read(&target).expect("read target") & ACCESS_BITS, 0o640);
}

#[test]
fn apply_cancellable_updates_directories_and_preserves_child_execute_bits() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("folder");
    let executable = root.join("run.sh");
    let plain = root.join("notes.txt");
    let dangling = root.join("dangling");
    fs::create_dir(&root).expect("create folder");
    fs::write(&executable, b"#!/bin/sh\n").expect("write script");
    fs::write(&plain, b"notes").expect("write plain");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("set root mode");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).expect("set script mode");
    fs::set_permissions(&plain, fs::Permissions::from_mode(0o600)).expect("set plain mode");
    symlink(root.join("missing"), &dangling).expect("create dangling symlink");

    let report = apply_cancellable(&root, 0o755, true, &CancellationToken::new());
    assert_eq!(report.succeeded, 3);
    assert!(report.failures.is_empty());
    assert_eq!(read(&root).expect("root mode") & ACCESS_BITS, 0o755);
    assert_eq!(read(&executable).expect("script mode") & ACCESS_BITS, 0o744);
    assert_eq!(read(&plain).expect("plain mode") & ACCESS_BITS, 0o644);
}

#[test]
fn apply_cancellable_stops_immediately_when_already_cancelled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("file.txt");
    fs::write(&file, b"content").expect("write file");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).expect("set file mode");

    let cancel = CancellationToken::new();
    cancel.cancel();
    let report = apply_cancellable(&file, 0o644, false, &cancel);
    assert_eq!(report.succeeded, 0);
    assert!(report.failures.is_empty());
    assert_eq!(read(&file).expect("read file") & ACCESS_BITS, 0o600);
}
