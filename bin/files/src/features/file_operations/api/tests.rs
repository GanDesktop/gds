use super::{Batch, Error, Kind};
use std::path::PathBuf;

#[test]
fn batch_deduplicates_targets_and_retains_partial_failures_without_stopping() {
    let root = tempfile::tempdir().expect("create operation fixture");
    let first = root.path().join("first");
    let denied = root.path().join("denied");
    let last = root.path().join("last");
    let batch = Batch::new(vec![
        first.clone(),
        denied.clone(),
        first.clone(),
        last.clone(),
    ])
    .expect("capture absolute selected entries");
    let mut seen = Vec::new();
    let report = batch.execute_with(Kind::Trash, |path| {
        seen.push(path.to_owned());
        if path == denied {
            Err(Error::Trash(trash::Error::Unknown {
                description: "fixture denied".into(),
            }))
        } else {
            Ok(())
        }
    });
    assert_eq!(seen, [first.clone(), denied.clone(), last.clone()]);
    assert_eq!(report.kind, Kind::Trash);
    assert_eq!(report.items.len(), 3);
    assert_eq!(report.succeeded_paths(), [first, last]);
    let failed: Vec<_> = report.failed_items().collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].path, denied);
}

#[test]
fn invalid_targets_are_rejected_before_a_batch_can_execute() {
    assert!(matches!(Batch::new(Vec::new()), Err(Error::EmptySelection)));
    assert!(matches!(
        Batch::new(vec![PathBuf::from("relative")]),
        Err(Error::InvalidTarget { .. })
    ));
    let root = tempfile::tempdir().expect("create validation fixture");
    let filesystem_root = root
        .path()
        .ancestors()
        .last()
        .expect("absolute path has a root")
        .to_path_buf();
    assert!(matches!(
        Batch::new(vec![filesystem_root]),
        Err(Error::InvalidTarget { .. })
    ));
    assert!(matches!(
        Batch::new(vec![root.path().join("..")]),
        Err(Error::InvalidTarget { .. })
    ));
}

#[cfg(unix)]
#[test]
fn symbolic_links_and_non_utf8_paths_are_not_replaced_with_display_or_target_paths() {
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };
    let root = tempfile::tempdir().expect("create original-path fixture");
    let target = root.path().join("target");
    std::fs::write(&target, b"keep target").expect("write target fixture");
    let link = root.path().join("link");
    symlink(&target, &link).expect("create symbolic-link fixture");
    let dangling = root.path().join("dangling");
    symlink(root.path().join("absent"), &dangling).expect("create dangling-link fixture");
    let invalid_utf8 = root.path().join(OsString::from_vec(b"raw-\xff".to_vec()));
    std::fs::write(&invalid_utf8, b"raw filename").expect("write non-UTF-8 fixture");
    let paths = vec![link.clone(), dangling.clone(), invalid_utf8.clone()];
    let batch = Batch::new(paths.clone()).expect("capture raw paths");
    assert!(
        matches!(batch.clipboard_text(), Err(Error::NonUtf8Clipboard { path }) if path == invalid_utf8)
    );
    let mut seen = Vec::new();
    let report = batch.execute_with(Kind::Trash, |path| {
        if path == link || path == dangling {
            assert!(
                std::fs::symlink_metadata(path)
                    .expect("read link without following")
                    .file_type()
                    .is_symlink()
            );
        }
        seen.push(path.to_owned());
        Ok(())
    });
    assert_eq!(seen, paths);
    assert_eq!(report.succeeded_paths(), paths);
    assert_eq!(
        std::fs::read(&target).expect("target is untouched"),
        b"keep target"
    );
}

#[cfg(unix)]
#[test]
fn permanent_delete_removes_entries_without_following_leaf_or_nested_links() {
    use std::{fs, os::unix::fs::symlink};

    let root = tempfile::tempdir().expect("create permanent-deletion fixture");
    let outside = root.path().join("outside");
    fs::create_dir(&outside).expect("create unselected target directory");
    let keep = outside.join("keep.txt");
    fs::write(&keep, b"keep target").expect("write unselected target");
    let entries = root.path().join("entries");
    fs::create_dir(&entries).expect("create selected entries directory");
    let alias = root.path().join("alias");
    symlink(&entries, &alias).expect("create parent-directory alias");
    let directory = entries.join("directory");
    fs::create_dir(&directory).expect("create selected directory");
    fs::write(directory.join("local.txt"), b"remove local").expect("write selected subtree");
    symlink(&outside, directory.join("nested-link")).expect("create nested external link");
    symlink(&keep, entries.join("leaf-link")).expect("create selected file link");
    symlink(outside.join("absent"), entries.join("dangling"))
        .expect("create dangling selected link");
    fs::write(entries.join("file.txt"), b"remove file").expect("write selected file");
    let paths: Vec<_> = ["directory", "leaf-link", "dangling", "file.txt"]
        .into_iter()
        .map(|name| alias.join(name))
        .collect();
    let report = Batch::new(paths.clone())
        .expect("freeze original alias paths")
        .execute(Kind::PermanentDelete);

    assert_eq!(report.kind, Kind::PermanentDelete);
    assert_eq!(report.succeeded_paths(), paths);
    assert_eq!(report.failed_items().count(), 0);
    assert_eq!(
        fs::read_dir(&entries)
            .expect("read emptied fixture")
            .count(),
        0
    );
    assert_eq!(
        fs::read(&keep).expect("unselected target remains"),
        b"keep target"
    );
    assert!(
        fs::symlink_metadata(&alias)
            .expect("parent alias remains")
            .file_type()
            .is_symlink()
    );
}
