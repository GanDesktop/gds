use super::*;

#[test]
fn disappeared_entry_is_retained_with_a_warning_but_inspection_fails() {
    let root = tempfile::tempdir().expect("create fixture directory");
    let path = root.path().join("file");
    fs::write(&path, "content").expect("write file");
    let entry = fs::read_dir(root.path())
        .expect("read directory")
        .next()
        .expect("single entry")
        .expect("directory entry");
    fs::remove_file(&path).expect("remove file before inspection");

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
        Entry::inspect(&path)
            .expect_err("missing entry must fail inspection")
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[cfg(unix)]
#[test]
fn symlink_targets_preserve_file_directory_broken_and_unreachable_states() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("create fixture directory");
    fs::write(root.path().join("source.rs"), "fn main() {}\n").expect("write source file");
    fs::create_dir(root.path().join("folder")).expect("create target directory");
    symlink("source.rs", root.path().join("file-link.png")).expect("create file symlink");
    symlink("folder", root.path().join("folder-link")).expect("create directory symlink");
    symlink("missing", root.path().join("broken-link")).expect("create broken symlink");
    symlink("loop-link", root.path().join("loop-link")).expect("create loop symlink");

    let snapshot =
        read_directory(root.path(), ReadOptions::default()).expect("read directory snapshot");
    assert!(snapshot.warnings.is_empty());

    let file_link = assert_symlink_entry(&snapshot, root.path(), "file-link.png");
    assert_eq!(
        file_link.details.target,
        Some(LinkTarget::Resolved {
            kind: EntryKind::File,
            content: Some(ContentType::new(
                ContentCategory::Code,
                Some("Rust"),
                Some("text/rust"),
            )),
        })
    );
    assert_eq!(
        file_link.details.display_content(),
        Some(&ContentType::new(
            ContentCategory::Code,
            Some("Rust"),
            Some("text/rust"),
        ))
    );

    let folder_link = assert_symlink_entry(&snapshot, root.path(), "folder-link");
    assert_eq!(
        folder_link.details.target,
        Some(LinkTarget::Resolved {
            kind: EntryKind::Directory,
            content: None,
        })
    );

    let broken_link = assert_symlink_entry(&snapshot, root.path(), "broken-link");
    assert_eq!(broken_link.details.target, Some(LinkTarget::Broken));

    let loop_link = assert_symlink_entry(&snapshot, root.path(), "loop-link");
    assert_eq!(loop_link.details.target, Some(LinkTarget::Unreachable));
}

#[cfg(unix)]
fn assert_symlink_entry<'a>(snapshot: &'a DirectorySnapshot, root: &Path, name: &str) -> &'a Entry {
    let listed = snapshot
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .expect("listed symlink entry");
    let inspected = Entry::inspect(&root.join(name)).expect("inspect symlink entry");
    assert_eq!(listed.kind, EntryKind::SymbolicLink);
    assert_eq!(inspected.kind, EntryKind::SymbolicLink);
    assert_eq!(listed.details.target, inspected.details.target);
    listed
}

#[test]
fn inspection_preserves_root_names_and_listing_filters_hidden_entries() {
    let root = tempfile::tempdir().expect("create fixture directory");
    fs::write(root.path().join(".hidden"), "content").expect("write hidden file");
    fs::create_dir(root.path().join("folder")).expect("create folder");
    let snapshot =
        read_directory(root.path(), ReadOptions::default()).expect("read directory snapshot");
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].kind, EntryKind::Directory);
    assert_eq!(
        snapshot.entries[0].details.children,
        Some(ChildCount {
            count: 0,
            truncated: false
        })
    );
    let hidden = Entry::inspect(&root.path().join(".hidden")).expect("inspect hidden file");
    assert!(hidden.is_hidden);
    assert_eq!(hidden.size, Some(7));

    let filesystem_root = root
        .path()
        .ancestors()
        .last()
        .expect("filesystem root ancestor");
    let entry = Entry::inspect(filesystem_root).expect("inspect filesystem root");
    assert_eq!(entry.name, filesystem_root.as_os_str());
    assert_eq!(entry.kind, EntryKind::Directory);
}
