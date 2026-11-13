use super::*;
#[test]
fn kde_format_has_no_csv_escaping() {
    assert_eq!(
        decode("工作,, Project/design,\"a,b\"".as_bytes()).unwrap(),
        ["工作", " Project/design", "\"a", "b\""]
    );
    assert!(validate("a,b").is_err());
}
#[test]
#[cfg(target_os = "linux")]
fn associations_and_copy_preserve_other_tags() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    std::fs::write(&a, "a").unwrap();
    std::fs::write(&b, "b").unwrap();
    change(&a, "工作/设计", true).unwrap();
    change(&a, "keep", true).unwrap();
    change(&a, "工作/设计", false).unwrap();
    copy(&a, &b).unwrap();
    assert_eq!(read(&b).unwrap(), ["keep"]);
    write(&a, &[]).unwrap();
    assert!(read_raw(&a).unwrap().is_none());
}
#[test]
#[cfg(target_os = "linux")]
fn editor_delta_retains_unrelated_external_changes() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let original = vec!["old".into(), "keep".into()];
    write(file.path(), &original).unwrap();
    change(file.path(), "external", true).unwrap();
    apply_edit(file.path(), &original, &["keep".into(), "new".into()]).unwrap();
    assert_eq!(read(file.path()).unwrap(), ["keep", "external", "new"]);
}

#[test]
#[cfg(target_os = "linux")]
fn apply_edit_works_on_directories_and_clears_removed_tags() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    let original = vec!["old".into(), "keep".into()];
    write(&folder, &original).unwrap();
    change(&folder, "external", true).unwrap();
    apply_edit(&folder, &original, &["keep".into(), "new".into()]).unwrap();
    assert_eq!(read(&folder).unwrap(), ["keep", "external", "new"]);

    let current = read(&folder).unwrap();
    apply_edit(&folder, &current, &[]).unwrap();
    assert!(read_raw(&folder).unwrap().is_none());
}

#[test]
#[cfg(target_os = "linux")]
fn apply_edit_rejects_symlinks_and_fifos_without_blocking_or_following() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    std::fs::write(&target, "content").unwrap();
    write(&target, &["preserved".into()]).unwrap();

    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let err = apply_edit(&link, &[], &["injected".into()]).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(read(&target).unwrap(), ["preserved"]);

    let dangling = dir.path().join("dangling");
    std::os::unix::fs::symlink(dir.path().join("missing"), &dangling).unwrap();
    let err = apply_edit(&dangling, &[], &["injected".into()]).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

    let fifo = dir.path().join("fifo");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
    )
    .unwrap();
    let err = apply_edit(&fifo, &[], &["injected".into()]).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
#[cfg(target_os = "linux")]
fn copy_from_writes_files_and_dirs_and_rejects_symlink_or_fifo_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::write(&source, "source").unwrap();
    write(&source, &["work".into(), "urgent".into()]).unwrap();
    let source_file = std::fs::File::open(&source).unwrap();

    let dest_file = dir.path().join("dest_file");
    std::fs::write(&dest_file, "dest").unwrap();
    copy_from(&source_file, &dest_file).unwrap();
    assert_eq!(read(&dest_file).unwrap(), ["work", "urgent"]);

    let dest_dir = dir.path().join("dest_dir");
    std::fs::create_dir(&dest_dir).unwrap();
    copy_from(&source_file, &dest_dir).unwrap();
    assert_eq!(read(&dest_dir).unwrap(), ["work", "urgent"]);

    let target = dir.path().join("target");
    std::fs::write(&target, "target").unwrap();
    write(&target, &["untouched".into()]).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let err = copy_from(&source_file, &link).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(read(&target).unwrap(), ["untouched"]);

    let fifo = dir.path().join("fifo");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
    )
    .unwrap();
    let err = copy_from(&source_file, &fifo).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
