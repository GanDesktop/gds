use super::*;
use directory::{ChildCount, EnrichmentNeed, EntryWarning, ReadOptions, WarningOperation};
use std::{
    io,
    sync::Arc,
    time::{Duration, SystemTime},
};

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

#[test]
fn full_rescan_uses_base_scan_and_reuses_unchanged_secondary_probes() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let untouched_dir = root.path().join("a-untouched-dir");
    std::fs::create_dir(&untouched_dir).expect("create untouched directory");
    std::fs::write(untouched_dir.join("child.txt"), "1").expect("write untouched child");

    let modified_dir = root.path().join("b-modified-dir");
    std::fs::create_dir(&modified_dir).expect("create modified directory");
    std::fs::write(modified_dir.join("first.txt"), "1").expect("write initial child");
    std::fs::File::open(&modified_dir)
        .and_then(|dir| dir.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)))
        .expect("backdate modified directory mtime");

    let untouched_blob = root.path().join("a-untouched-blob");
    std::fs::write(&untouched_blob, b"\x89PNG\r\n\x1a\nheader").expect("write untouched blob");

    let removed_file = root.path().join("z-removed.txt");
    std::fs::write(&removed_file, "remove me").expect("write removed file");

    let before =
        directory::read_directory(root.path(), ReadOptions::default()).expect("read directory");

    std::fs::remove_file(&removed_file).expect("remove initial file");
    std::fs::write(modified_dir.join("second.txt"), "2").expect("add child to modified directory");

    let added_dir = root.path().join("c-added-dir");
    std::fs::create_dir(&added_dir).expect("create added directory");
    std::fs::write(added_dir.join("inside.txt"), "1").expect("write child in added directory");

    let added_blob = root.path().join("c-added-blob");
    std::fs::write(&added_blob, b"\x89PNG\r\n\x1a\nheader").expect("write added blob");

    let mut changes = Changes::default();
    (0..=MAX_PATHS).for_each(|index| {
        let bulk_path = root.path().join(format!("m-bulk-{index:04}.txt"));
        std::fs::write(&bulk_path, "bulk").expect("write bulk file");
        changes.push(
            notify::Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path(bulk_path),
        );
    });

    let after = changes.apply(before).expect("apply full rescan");
    assert!(after.entry_for_path(&removed_file).is_none());
    assert_eq!(
        after
            .entries
            .iter()
            .take(3)
            .map(|entry| entry.display_name.as_str())
            .collect::<Vec<_>>(),
        ["a-untouched-dir", "b-modified-dir", "c-added-dir"]
    );

    let children_need = EnrichmentNeed {
        children: true,
        ..EnrichmentNeed::default()
    };
    let content_need = EnrichmentNeed {
        content: true,
        ..EnrichmentNeed::default()
    };

    let untouched_dir_entry = after
        .entry_for_path(&untouched_dir)
        .expect("untouched directory entry");
    assert_eq!(
        untouched_dir_entry.details.children,
        Some(ChildCount {
            count: 1,
            truncated: false,
        })
    );
    assert!(!untouched_dir_entry.needs_enrichment(children_need));

    let untouched_blob_entry = after
        .entry_for_path(&untouched_blob)
        .expect("untouched blob entry");
    assert_eq!(
        untouched_blob_entry
            .details
            .content
            .as_ref()
            .and_then(|content| content.mime.as_deref()),
        Some("image/png")
    );
    assert!(!untouched_blob_entry.needs_enrichment(content_need));

    let modified_dir_entry = after
        .entry_for_path(&modified_dir)
        .expect("modified directory entry");
    assert_eq!(modified_dir_entry.details.children, None);
    assert!(modified_dir_entry.needs_enrichment(children_need));

    let added_dir_entry = after
        .entry_for_path(&added_dir)
        .expect("added directory entry");
    assert_eq!(added_dir_entry.details.children, None);
    assert!(added_dir_entry.needs_enrichment(children_need));

    let added_blob_entry = after.entry_for_path(&added_blob).expect("added blob entry");
    assert_eq!(added_blob_entry.details.content, None);
    assert!(added_blob_entry.needs_enrichment(content_need));
}
