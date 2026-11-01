use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    fs,
    path::PathBuf,
};

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use super::{Collision, Error, NameError, NamedTarget, collisions, free_name, numbered, validate};

fn name(value: &str) -> OsString {
    OsString::from(value)
}

#[test]
fn numbered_names_keep_the_extension_and_append_after_the_stem() {
    assert_eq!(
        numbered(OsStr::new("report.pdf"), 2),
        name("report (2).pdf")
    );
    assert_eq!(
        numbered(OsStr::new("archive.tar.gz"), 3),
        name("archive.tar (3).gz")
    );
    assert_eq!(numbered(OsStr::new("notes"), 2), name("notes (2)"));
    assert_eq!(numbered(OsStr::new(".env"), 2), name(".env (2)"));
    assert_eq!(numbered(OsStr::new("..notes"), 2), name("..notes (2)"));
    assert_eq!(numbered(OsStr::new("trailing."), 2), name("trailing (2)."));
}

#[test]
fn a_leaf_name_is_validated_without_trimming_or_policy_guesses() {
    for invalid in ["", ".", "..", "a/b"] {
        assert!(
            validate(OsStr::new(invalid)).is_err(),
            "{invalid:?} must be rejected"
        );
    }
    assert_eq!(validate(OsStr::new("")), Err(NameError::Empty));
    assert_eq!(validate(OsStr::new(".")), Err(NameError::Reserved));
    assert_eq!(validate(OsStr::new("..")), Err(NameError::Reserved));
    assert_eq!(validate(OsStr::new("a/b")), Err(NameError::Separator));
    assert_eq!(validate(OsStr::new("a\0b")), Err(NameError::Invalid));
    assert_eq!(validate(OsStr::new(" leading and trailing ")), Ok(()));
    assert_eq!(validate(OsStr::new("…⁉")), Ok(()));
}

#[test]
fn free_names_skip_existing_and_reserved_candidates() {
    let root = tempfile::tempdir().expect("create naming fixture");
    let directory = root.path();
    fs::write(directory.join("report.pdf"), b"existing").expect("write existing target");
    fs::write(directory.join("report (2).pdf"), b"existing").expect("write numbered target");
    assert_eq!(
        free_name(directory, OsStr::new("report.pdf"), &HashSet::new()).expect("free name"),
        name("report (3).pdf")
    );
    let reserved = HashSet::from([name("report (3).pdf")]);
    assert_eq!(
        free_name(directory, OsStr::new("report.pdf"), &reserved).expect("free name"),
        name("report (4).pdf")
    );
    assert_eq!(
        free_name(directory, OsStr::new("unused.txt"), &HashSet::new()).expect("free name"),
        name("unused.txt")
    );
    assert!(matches!(
        free_name(directory, OsStr::new(".."), &HashSet::new()),
        Err(Error::Name(NameError::Reserved))
    ));
}

#[cfg(unix)]
#[test]
fn non_utf8_leaves_keep_their_bytes_in_suggestions_and_collisions() {
    let root = tempfile::tempdir().expect("create raw naming fixture");
    let directory = root.path();
    let raw = OsString::from_vec(b"raw-\xff.txt".to_vec());
    fs::write(directory.join(&raw), b"existing").expect("write raw target");
    let suggested = free_name(directory, &raw, &HashSet::new()).expect("free raw name");
    assert_eq!(suggested.as_os_str().as_bytes(), &b"raw-\xff (2).txt"[..]);
    let source = directory.join(&raw);
    let found = collisions(directory, std::slice::from_ref(&source), &[]).expect("scan");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].target.as_os_str().as_bytes(), raw.as_bytes());
}

#[test]
fn collisions_report_existing_targets_and_later_duplicate_sources() {
    let root = tempfile::tempdir().expect("create collision fixture");
    let first = root.path().join("first").join("taken");
    let second = root.path().join("second").join("taken");
    let free = root.path().join("third").join("free");
    fs::create_dir(root.path().join("destination")).expect("create destination");
    fs::write(root.path().join("destination/taken"), b"target").expect("write target");
    let sources = vec![first.clone(), free.clone(), second.clone()];

    let found =
        collisions(&root.path().join("destination"), &sources, &[]).expect("scan planned names");
    assert_eq!(
        found,
        vec![
            Collision {
                source: first.clone(),
                target: name("taken")
            },
            Collision {
                source: second.clone(),
                target: name("taken")
            },
        ]
    );

    let chosen = [NamedTarget::new(first.clone(), name("taken (2)"))];
    let found = collisions(&root.path().join("destination"), &sources, &chosen)
        .expect("scan renamed source");
    assert_eq!(
        found,
        vec![Collision {
            source: second,
            target: name("taken")
        }],
        "the renamed source no longer collides and the free source stays free"
    );
}

#[test]
fn a_free_suggestion_resolves_a_duplicate_target_in_order() {
    let root = tempfile::tempdir().expect("create duplicate fixture");
    fs::create_dir(root.path().join("destination")).expect("create destination");
    let first = PathBuf::from("/source/a/report.pdf");
    let second = PathBuf::from("/source/b/report.pdf");
    let sources = vec![first.clone(), second.clone()];
    let destination = root.path().join("destination");

    let found = collisions(&destination, &sources, &[]).expect("scan duplicate names");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].source, second.clone());

    // The suggestion reserves the other pending source's planned leaf.
    let reserved = HashSet::from([OsString::from("report.pdf")]);
    let suggestion =
        free_name(&destination, OsStr::new("report.pdf"), &reserved).expect("free suggestion");
    assert_eq!(suggestion, OsString::from("report (2).pdf"));
    let chosen = [NamedTarget::new(second, suggestion)];
    assert!(
        collisions(&destination, &sources, &chosen)
            .expect("scan chosen name")
            .is_empty(),
        "one renamed leaf resolves the shared target"
    );
}
