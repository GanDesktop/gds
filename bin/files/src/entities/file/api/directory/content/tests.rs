use super::*;
use std::fs;

fn database() -> SharedMimeInfo {
    let root = tempfile::tempdir().expect("create MIME test directory");
    let mime = root.path().join("mime");
    fs::create_dir(&mime).expect("create mime subdirectory");
    fs::write(
        mime.join("globs2"),
        "50:text/rust:*.source-test\n\
         50:image/png:*.png\n\
         50:image/jpeg:*.jpg\n\
         50:application/vnd.openxmlformats-officedocument.wordprocessingml.document:*.docx\n\
         50:image/x-test-png:*.alias\n\
         50:text/x-makefile:Makefile\n",
    )
    .expect("write globs2 fixture");
    fs::write(mime.join("aliases"), "image/x-test-png image/png\n").expect("write aliases fixture");
    // Minimal shared-mime-info magic file: priority, MIME, offset, length, bytes.
    fs::write(
        mime.join("magic"),
        b"MIME-Magic\0\n[80:image/png]\n>0=\0\x08\x89PNG\r\n\x1a\n\n[50:application/zip]\n>0=\0\x04PK\x03\x04\n",
    )
    .expect("write magic fixture");
    SharedMimeInfo::new_for_directory(root.path())
}

fn assert_detected(
    database: &SharedMimeInfo,
    name: &str,
    path: &Path,
    category: ContentCategory,
    format: Option<&str>,
    mime: &str,
) {
    let content = ContentType::detect_with_database(database, OsStr::new(name), path)
        .expect("content type should be detected");
    assert_eq!(content.category, category);
    assert_eq!(content.format.as_deref(), format);
    assert_eq!(content.mime.as_deref(), Some(mime));
}

#[test]
fn database_patterns_and_aliases_keep_short_names_and_categories() {
    let database = database();
    let root = tempfile::tempdir().expect("create fixture directory");
    let missing = root.path().join("missing");
    assert_detected(
        &database,
        "main.source-test",
        &missing,
        ContentCategory::Code,
        Some("Rust"),
        "text/rust",
    );
    assert_detected(
        &database,
        "Makefile",
        &missing,
        ContentCategory::Code,
        Some("Makefile"),
        "text/x-makefile",
    );
    assert_detected(
        &database,
        "picture.alias",
        &missing,
        ContentCategory::Image,
        Some("PNG"),
        "image/png",
    );
}

#[test]
fn recognized_names_take_precedence_over_file_headers() {
    let database = database();
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("sample");
    fs::write(&path, b"\x89PNG\r\n\x1a\nrest").expect("write sample header");
    assert_detected(
        &database,
        "picture.jpg",
        &path,
        ContentCategory::Image,
        Some("JPEG"),
        "image/jpeg",
    );
    assert_detected(
        &database,
        "main.source-test",
        &path,
        ContentCategory::Code,
        Some("Rust"),
        "text/rust",
    );
}

#[test]
fn unknown_names_fall_back_to_file_headers() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("sample");
    fs::write(&path, b"\x89PNG\r\n\x1a\nrest").expect("write PNG header");
    assert_detected(
        &database(),
        "no-extension",
        &path,
        ContentCategory::Image,
        Some("PNG"),
        "image/png",
    );
}

#[test]
fn unavailable_and_unrecognized_content_keep_existing_fallbacks() {
    let database = database();
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("sample");
    assert!(ContentType::detect_with_database(&database, OsStr::new("unknown"), &path).is_none());

    fs::write(&path, b"").expect("write empty file");
    assert_detected(
        &database,
        "unknown",
        &path,
        ContentCategory::Text,
        None,
        "text/plain",
    );

    fs::write(&path, b"plain text").expect("write text file");
    assert_detected(
        &database,
        "unknown",
        &path,
        ContentCategory::Text,
        None,
        "text/plain",
    );

    fs::write(&path, b"\0\xff\0\xff").expect("write binary file");
    assert_detected(
        &database,
        "unknown",
        &path,
        ContentCategory::Binary,
        None,
        "application/octet-stream",
    );
}

#[test]
fn content_reads_remain_bounded() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("large-file");
    fs::write(&path, vec![b'a'; HEADER_LEN as usize + 1024]).expect("write large file");
    assert_eq!(
        ContentType::read_header(&path)
            .expect("read bounded header")
            .len(),
        HEADER_LEN as usize
    );
}
