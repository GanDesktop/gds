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

#[test]
#[cfg(target_os = "linux")]
fn copy_from_preserves_portable_user_xattrs_and_reads_origin_and_mime() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("downloaded.bin");
    std::fs::write(&source, b"hello").unwrap();
    write(&source, &["work".into()]).unwrap();
    rustix::fs::setxattr(
        &source,
        ORIGIN_URL_ATTRIBUTE,
        b"https://example.com/files/report.pdf",
        rustix::fs::XattrFlags::empty(),
    )
    .unwrap();
    rustix::fs::setxattr(
        &source,
        MIME_TYPE_ATTRIBUTE,
        b"application/pdf",
        rustix::fs::XattrFlags::empty(),
    )
    .unwrap();
    rustix::fs::setxattr(
        &source,
        "user.xdg.comment",
        b"Quarterly summary",
        rustix::fs::XattrFlags::empty(),
    )
    .unwrap();

    assert_eq!(
        read_origin_url(&source).as_deref(),
        Some("https://example.com/files/report.pdf")
    );
    assert_eq!(read_mime_type(&source).as_deref(), Some("application/pdf"));

    let dest = dir.path().join("copied.bin");
    std::fs::write(&dest, b"hello").unwrap();
    let source_file = std::fs::File::open(&source).unwrap();
    copy_from(&source_file, &dest).unwrap();

    assert_eq!(read(&dest).unwrap(), ["work"]);
    assert_eq!(
        read_origin_url(&dest).as_deref(),
        Some("https://example.com/files/report.pdf")
    );
    assert_eq!(read_mime_type(&dest).as_deref(), Some("application/pdf"));
    assert_eq!(
        read_named_raw(&dest, "user.xdg.comment").as_deref(),
        Some(b"Quarterly summary".as_slice())
    );
}

#[test]
#[cfg(target_os = "linux")]
fn comment_round_trip_clears_validates_and_rejects_symlinks_and_fifos() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("doc.txt");
    std::fs::write(&file, b"hello").unwrap();

    assert_eq!(read_comment(&file).unwrap(), None);

    write_comment(&file, Some("  Initial review note  ")).unwrap();
    assert_eq!(
        read_comment(&file).unwrap().as_deref(),
        Some("Initial review note")
    );

    // Clearing with empty or whitespace removes the attribute.
    write_comment(&file, Some("   ")).unwrap();
    assert_eq!(read_comment(&file).unwrap(), None);
    assert!(read_named_raw(&file, COMMENT_ATTRIBUTE).is_none());

    // Validation rejects NUL bytes and oversized payloads.
    let nul_err = write_comment(&file, Some("bad\0note")).unwrap_err();
    assert_eq!(nul_err.kind(), io::ErrorKind::InvalidInput);
    let big = "a".repeat(MAX_COMMENT_BYTES + 1);
    let size_err = write_comment(&file, Some(&big)).unwrap_err();
    assert_eq!(size_err.kind(), io::ErrorKind::InvalidInput);

    // Reject symlinks without following them and FIFOs without blocking.
    let link = dir.path().join("link.txt");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    let link_err = write_comment(&link, Some("via link")).unwrap_err();
    assert_eq!(link_err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(read_comment(&file).unwrap(), None);

    let fifo = dir.path().join("pipe");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
    )
    .unwrap();
    let fifo_err = write_comment(&fifo, Some("on fifo")).unwrap_err();
    assert_eq!(fifo_err.kind(), io::ErrorKind::InvalidInput);
}
