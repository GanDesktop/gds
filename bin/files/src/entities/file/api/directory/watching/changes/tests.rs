use super::*;
use directory::ReadOptions;
#[::core::prelude::v1::test]
fn external_xattr_change_updates_metadata_without_changing_file_mtime() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("file");
    std::fs::write(&path, "content").unwrap();
    let before = directory::read_directory(root.path(), ReadOptions::default()).unwrap();
    gds::service::file_tags::write(&path, &["external".into()]).unwrap();
    let mut changes = Changes::default();
    changes.push(
        notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Metadata(
            notify::event::MetadataKind::Extended,
        )))
        .add_path(path),
    );
    let after = changes.apply(before.clone()).unwrap();
    assert_eq!(before.entries[0].modified, after.entries[0].modified);
    assert_ne!(
        before.entries[0].details.changed,
        after.entries[0].details.changed
    );
}
