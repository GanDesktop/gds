use super::*;
use std::fs;

fn database() -> SharedMimeInfo {
    let root = tempfile::tempdir().unwrap();
    let mime = root.path().join("mime");
    fs::create_dir(&mime).unwrap();
    fs::write(
        mime.join("globs2"),
        "50:text/rust:*.source-test\n\
         50:image/png:*.png\n\
         50:image/jpeg:*.jpg\n\
         50:application/vnd.openxmlformats-officedocument.wordprocessingml.document:*.docx\n\
         50:image/x-test-png:*.alias\n\
         50:text/x-makefile:Makefile\n",
    )
    .unwrap();
    fs::write(mime.join("aliases"), "image/x-test-png image/png\n").unwrap();
    // Minimal shared-mime-info magic file: priority, MIME, offset, length, bytes.
    fs::write(
        mime.join("magic"),
        b"MIME-Magic\0\n[80:image/png]\n>0=\0\x08\x89PNG\r\n\x1a\n\n[50:application/zip]\n>0=\0\x04PK\x03\x04\n",
    ).unwrap();
    SharedMimeInfo::new_for_directory(root.path())
}

#[test]
fn database_patterns_and_aliases_keep_short_names_and_categories() {
    let database = database();
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing");
    for (name, category, format, mime) in [
        (
            "main.source-test",
            ContentCategory::Code,
            "Rust",
            "text/rust",
        ),
        (
            "Makefile",
            ContentCategory::Code,
            "Makefile",
            "text/x-makefile",
        ),
        ("picture.alias", ContentCategory::Image, "PNG", "image/png"),
    ] {
        let content =
            ContentType::detect_with_database(&database, OsStr::new(name), &missing).unwrap();
        assert_eq!(content.category, category);
        assert_eq!(content.format.as_deref(), Some(format));
        assert_eq!(content.mime.as_deref(), Some(mime));
    }
}

#[test]
fn recognized_names_take_precedence_over_file_headers() {
    let database = database();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("sample");
    fs::write(&path, b"\x89PNG\r\n\x1a\nrest").unwrap();
    for (name, category, format, mime) in [
        ("picture.jpg", ContentCategory::Image, "JPEG", "image/jpeg"),
        (
            "main.source-test",
            ContentCategory::Code,
            "Rust",
            "text/rust",
        ),
    ] {
        let content =
            ContentType::detect_with_database(&database, OsStr::new(name), &path).unwrap();
        assert_eq!(content.category, category);
        assert_eq!(content.format.as_deref(), Some(format));
        assert_eq!(content.mime.as_deref(), Some(mime));
    }
}

#[test]
fn unknown_names_fall_back_to_file_headers() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("sample");
    fs::write(&path, b"\x89PNG\r\n\x1a\nrest").unwrap();
    let content =
        ContentType::detect_with_database(&database(), OsStr::new("no-extension"), &path).unwrap();
    assert_eq!(content.category, ContentCategory::Image);
    assert_eq!(content.format.as_deref(), Some("PNG"));
    assert_eq!(content.mime.as_deref(), Some("image/png"));
}

#[test]
fn unavailable_and_unrecognized_content_keep_existing_fallbacks() {
    let database = database();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("sample");
    assert!(ContentType::detect_with_database(&database, OsStr::new("unknown"), &path).is_none());
    for (header, category, mime) in [
        (&b""[..], ContentCategory::Text, "text/plain"),
        (&b"plain text"[..], ContentCategory::Text, "text/plain"),
        (
            &b"\0\xff\0\xff"[..],
            ContentCategory::Binary,
            "application/octet-stream",
        ),
    ] {
        fs::write(&path, header).unwrap();
        let content =
            ContentType::detect_with_database(&database, OsStr::new("unknown"), &path).unwrap();
        assert_eq!(content.category, category);
        assert_eq!(content.mime.as_deref(), Some(mime));
    }
}

#[test]
fn content_reads_remain_bounded() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("large-file");
    fs::write(&path, vec![b'a'; HEADER_LEN as usize + 1024]).unwrap();
    assert_eq!(read_header(&path).unwrap().len(), HEADER_LEN as usize);
}
