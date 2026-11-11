use super::*;

#[test]
fn disappeared_entry_is_retained_with_a_warning_but_inspection_fails() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("file");
    fs::write(&path, "content").unwrap();
    let entry = fs::read_dir(root.path()).unwrap().next().unwrap().unwrap();
    fs::remove_file(&path).unwrap();

    let mut warnings = Vec::new();
    let entry = Entry::from_dir_entry(entry, &mut IdentityCache::new(), &mut warnings);
    assert_eq!(entry.path, path);
    assert_eq!(entry.name, "file");
    assert_eq!(entry.size, None);
    assert_eq!(entry.modified, None);
    assert_eq!(entry.details, EntryDetails::default());
    assert!(warnings.iter().any(|warning| {
        warning.path == path
            && warning.operation == WarningOperation::Metadata
            && warning.source.kind() == io::ErrorKind::NotFound
    }));
    assert_eq!(
        Entry::inspect(&path).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
}

#[cfg(unix)]
#[test]
fn symlink_targets_preserve_file_directory_broken_and_unreachable_states() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("source.rs"), "fn main() {}\n").unwrap();
    fs::create_dir(root.path().join("folder")).unwrap();
    for (name, target) in [
        ("file-link.png", "source.rs"),
        ("folder-link", "folder"),
        ("broken-link", "missing"),
        ("loop-link", "loop-link"),
    ] {
        symlink(target, root.path().join(name)).unwrap();
    }

    let snapshot = read_directory(root.path(), ReadOptions::default()).unwrap();
    assert!(snapshot.warnings.is_empty());
    for name in ["file-link.png", "folder-link", "broken-link", "loop-link"] {
        let listed = snapshot
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap();
        let inspected = Entry::inspect(&root.path().join(name)).unwrap();
        assert_eq!(listed.kind, EntryKind::SymbolicLink);
        assert_eq!(inspected.kind, EntryKind::SymbolicLink);
        assert_eq!(listed.details.target, inspected.details.target);
        match name {
            "file-link.png" => {
                let Some(LinkTarget::Resolved {
                    kind: EntryKind::File,
                    content: Some(content),
                }) = &listed.details.target
                else {
                    panic!("expected a resolved file with content metadata");
                };
                assert_eq!(listed.details.display_content(), Some(content));
            }
            "folder-link" => assert!(matches!(
                listed.details.target,
                Some(LinkTarget::Resolved {
                    kind: EntryKind::Directory,
                    content: None
                })
            )),
            "broken-link" => assert_eq!(listed.details.target, Some(LinkTarget::Broken)),
            "loop-link" => assert_eq!(listed.details.target, Some(LinkTarget::Unreachable)),
            _ => unreachable!(),
        }
    }
}

#[test]
fn inspection_preserves_root_names_and_listing_filters_hidden_entries() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".hidden"), "content").unwrap();
    fs::create_dir(root.path().join("folder")).unwrap();
    let snapshot = read_directory(root.path(), ReadOptions::default()).unwrap();
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].kind, EntryKind::Directory);
    assert_eq!(
        snapshot.entries[0].details.children,
        Some(ChildCount {
            count: 0,
            truncated: false
        })
    );
    let hidden = Entry::inspect(&root.path().join(".hidden")).unwrap();
    assert!(hidden.is_hidden);
    assert_eq!(hidden.size, Some(7));

    let filesystem_root = root.path().ancestors().last().unwrap();
    let entry = Entry::inspect(filesystem_root).unwrap();
    assert_eq!(entry.name, filesystem_root.as_os_str());
    assert_eq!(entry.kind, EntryKind::Directory);
}
