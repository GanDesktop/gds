use std::{ffi::OsStr, fs, path::Path};

use super::{Error, NameError, Request, validate_name};

#[test]
fn rejects_names_that_are_not_a_single_leaf() {
    let current = OsStr::new("old.txt");
    assert_eq!(validate_name("", current), Err(NameError::Empty));
    assert_eq!(validate_name("a/b", current), Err(NameError::Separator));
    assert_eq!(validate_name(".", current), Err(NameError::Reserved));
    assert_eq!(validate_name("..", current), Err(NameError::Reserved));
    assert_eq!(validate_name("old.txt", current), Err(NameError::Unchanged));
    assert_eq!(validate_name("bad\0name", current), Err(NameError::Invalid));
    assert_eq!(validate_name("new.txt", current), Ok(()));
    // Surrounding spaces are legal and never trimmed.
    assert_eq!(validate_name(" new.txt ", current), Ok(()));
}

#[test]
fn refuses_to_replace_an_existing_entry() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let source = directory.path().join("source.txt");
    let existing = directory.path().join("target.txt");
    fs::write(&source, b"source").expect("write source");
    fs::write(&existing, b"target").expect("write target");

    let request = Request {
        source: source.clone(),
        name: "target.txt".into(),
    };
    let error = request.execute().expect_err("target must not be replaced");
    assert!(matches!(error, Error::Exists { .. }));
    assert_eq!(fs::read(&source).expect("source still exists"), b"source");
    assert_eq!(fs::read(&existing).expect("target is untouched"), b"target");
}

#[cfg(target_os = "linux")]
#[test]
fn renames_without_touching_contents() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let source = directory.path().join("before.txt");
    fs::write(&source, b"payload").expect("write source");

    let request = Request {
        source: source.clone(),
        name: "after.txt".into(),
    };
    let destination = request.execute().expect("rename succeeds");
    assert_eq!(destination.file_name(), Some(OsStr::new("after.txt")));
    assert!(!source.exists());
    assert_eq!(fs::read(&destination).expect("renamed file"), b"payload");
}

#[cfg(target_os = "linux")]
#[test]
fn rename_keeps_the_entry_in_the_same_parent() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let source = directory.path().join("before.txt");
    fs::write(&source, b"payload").expect("write source");

    let request = Request {
        source: source.clone(),
        name: "after.txt".into(),
    };
    let destination = request.execute().expect("rename succeeds");
    assert_eq!(destination.parent(), Some(Path::new(directory.path())));
}
