use super::*;
use directory::{EntryWarning, ReadOptions, WarningOperation};
use std::{io, sync::Arc};

#[test]
fn external_xattr_change_updates_metadata_and_keeps_canonical_entry_order() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("z-file");
    std::fs::write(&path, "content").expect("write initial file");
    let mut before =
        directory::read_directory(root.path(), ReadOptions::default()).expect("read directory");
    before.warnings.push(EntryWarning {
        path: path.clone(),
        operation: WarningOperation::Metadata,
        source: Arc::new(io::Error::from(io::ErrorKind::NotFound)),
    });
    gds::service::file_tags::write(&path, &["external".into()]).expect("write xattr tag");
    let added_file = root.path().join("a-file");
    std::fs::write(&added_file, "added").expect("write added file");
    let added_dir = root.path().join("m-folder");
    std::fs::create_dir(&added_dir).expect("create added folder");

    let mut changes = Changes::default();
    changes.push(
        notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Metadata(
            notify::event::MetadataKind::Extended,
        )))
        .add_path(path.clone())
        .add_path(added_file)
        .add_path(added_dir),
    );
    let after = changes.apply(before.clone()).expect("apply watch changes");
    assert!(after.warnings.is_empty());
    let names: Vec<_> = after
        .entries
        .iter()
        .map(|entry| entry.display_name.as_str())
        .collect();
    assert_eq!(names, ["m-folder", "a-file", "z-file"]);
    assert_eq!(before.entries[0].modified, after.entries[2].modified);
    assert_ne!(
        before.entries[0].details.changed,
        after.entries[2].details.changed
    );
}
