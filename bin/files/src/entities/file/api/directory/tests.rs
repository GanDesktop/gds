use super::children;
use super::identity::IdentityCache;
use super::*;

/// A real 1x1 PNG, small enough to inline; it must be recognised by content,
/// never by the file name it is stored under.
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn names(snapshot: &DirectorySnapshot) -> Vec<OsString> {
    snapshot
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect()
}

fn named<'a>(snapshot: &'a DirectorySnapshot, name: &str) -> &'a Entry {
    let name = OsString::from(name);
    snapshot
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .expect("fixture entry is present")
}

#[test]
fn lists_direct_children_with_directories_first_and_real_metadata() {
    let root = tempfile::tempdir().expect("create directory fixture");
    fs::create_dir(root.path().join("z-folder")).expect("create folder");
    fs::create_dir(root.path().join("a-folder")).expect("create folder");
    fs::write(root.path().join("z-folder/nested"), b"not a direct child")
        .expect("create nested file");
    fs::write(root.path().join("a-file"), b"hello").expect("create file");
    fs::write(root.path().join("z-file"), b"world").expect("create file");

    let snapshot = read_directory(root.path(), ReadOptions::default()).expect("read fixture");

    assert_eq!(snapshot.path, root.path());
    assert_eq!(
        names(&snapshot),
        ["a-folder", "z-folder", "a-file", "z-file"]
            .map(OsString::from)
            .to_vec()
    );
    assert!(snapshot.warnings.is_empty());
    assert_eq!(snapshot.entries[0].kind, EntryKind::Directory);
    assert_eq!(snapshot.entries[0].size, None);
    assert_eq!(snapshot.entries[2].kind, EntryKind::File);
    assert_eq!(snapshot.entries[2].size, Some(5));
    assert!(snapshot.entries[2].modified.is_some());
}

#[test]
fn hidden_files_are_excluded_by_default_and_can_be_shown() {
    let root = tempfile::tempdir().expect("create directory fixture");
    fs::write(root.path().join(".hidden"), b"hidden").expect("create hidden file");
    fs::create_dir(root.path().join(".config")).expect("create hidden directory");
    fs::write(root.path().join("visible"), b"visible").expect("create visible file");

    let default = read_directory(root.path(), ReadOptions::default()).expect("read visible items");
    assert_eq!(names(&default), vec![OsString::from("visible")]);

    let all =
        read_directory(root.path(), ReadOptions { show_hidden: true }).expect("read hidden items");
    assert_eq!(all.entries.len(), 3);
    assert_eq!(
        all.entries.iter().filter(|entry| entry.is_hidden).count(),
        2
    );
}

#[test]
fn missing_target_and_file_target_are_errors_not_empty_listings() {
    let root = tempfile::tempdir().expect("create directory fixture");
    let missing = read_directory(&root.path().join("missing"), ReadOptions::default());
    assert!(matches!(missing, Err(Error::OpenDirectory { .. })));
    let file = root.path().join("file");
    fs::write(&file, b"not a directory").expect("create file target");
    assert!(matches!(
        read_directory(&file, ReadOptions::default()),
        Err(Error::OpenDirectory { .. })
    ));
}

#[test]
fn directory_rows_report_direct_child_counts_including_hidden_entries() {
    let root = tempfile::tempdir().expect("create directory fixture");
    fs::create_dir(root.path().join("folder")).expect("create folder");
    fs::write(root.path().join("folder/visible"), b"x").expect("create child file");
    fs::write(root.path().join("folder/.hidden"), b"x").expect("create hidden child file");
    fs::create_dir(root.path().join("folder/nested")).expect("create child folder");
    fs::create_dir(root.path().join("empty")).expect("create empty folder");
    fs::write(root.path().join("plain.txt"), b"data").expect("create plain file");

    let snapshot = read_directory(root.path(), ReadOptions::default()).expect("read fixture");

    assert_eq!(
        named(&snapshot, "folder").details.children,
        Some(ChildCount {
            count: 3,
            truncated: false,
        })
    );
    assert_eq!(
        named(&snapshot, "empty").details.children,
        Some(ChildCount {
            count: 0,
            truncated: false,
        })
    );
    assert!(named(&snapshot, "plain.txt").details.children.is_none());
}

#[test]
fn child_counting_is_bounded_and_exact_below_the_limit() {
    let root = tempfile::tempdir().expect("create directory fixture");
    for name in ["a", "b", "c"] {
        fs::write(root.path().join(name), b"x").expect("create child file");
    }

    assert_eq!(
        children::count_with_limit(root.path(), 3),
        Some(ChildCount {
            count: 3,
            truncated: false,
        })
    );
    assert_eq!(
        children::count_with_limit(root.path(), 2),
        Some(ChildCount {
            count: 2,
            truncated: true,
        })
    );
}

#[test]
fn enumeration_failure_reports_unknown_instead_of_a_partial_count() {
    let unreadable = || io::Error::other("directory entry unreadable");
    let entries = vec![Ok::<(), io::Error>(()), Err(unreadable())];

    assert_eq!(children::count_entries(entries, 1000), None);
}

#[test]
fn uncountable_directory_reports_unknown_instead_of_zero() {
    let root = tempfile::tempdir().expect("create directory fixture");
    let file = root.path().join("plain.txt");
    fs::write(&file, b"data").expect("create plain file");

    assert_eq!(children::count(&file), None);
    assert_eq!(children::count(&root.path().join("missing")), None);
}

#[cfg(unix)]
#[test]
fn linked_directories_keep_their_link_identity_and_no_child_count() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("create directory fixture");
    let target = tempfile::tempdir().expect("create linked directory");
    fs::write(target.path().join("child"), b"outside listing").expect("create linked content");
    symlink(target.path(), root.path().join("folder-link")).expect("create directory link");

    let snapshot = read_directory(root.path(), ReadOptions::default()).expect("read fixture");
    let link = named(&snapshot, "folder-link");

    assert_eq!(link.kind, EntryKind::SymbolicLink);
    assert_eq!(link.size, None);
    assert_eq!(link.details.children, None);
}

#[test]
fn metadata_failure_keeps_the_entry_and_reports_its_path() {
    let root = tempfile::tempdir().expect("create directory fixture");
    let path = root.path().join("removed-after-enumeration");
    fs::write(&path, b"data").expect("create file");
    let entry = fs::read_dir(root.path())
        .expect("open fixture")
        .next()
        .expect("fixture has an entry")
        .expect("read entry");
    fs::remove_file(&path).expect("remove the fixture file after enumeration");

    let mut warnings = Vec::new();
    let mut identities = IdentityCache::new();
    let item = read_entry(entry, false, &mut identities, &mut warnings);

    assert_eq!(item.path, path);
    assert_eq!(item.name, OsString::from("removed-after-enumeration"));
    assert_eq!(item.size, None);
    assert_eq!(item.modified, None);
    assert!(item.details.owner.is_none());
    assert!(item.details.permissions.is_none());
    assert!(item.details.content.is_none());
    assert!(warnings.iter().any(|warning| {
        warning.path == path
            && warning.operation == WarningOperation::Metadata
            && warning.error_kind == io::ErrorKind::NotFound
    }));
}

#[test]
fn enumeration_failure_is_retained_even_if_no_entries_can_be_returned() {
    let root = tempfile::tempdir().expect("create directory fixture");
    let entries = std::iter::once(Err(io::Error::from(io::ErrorKind::PermissionDenied)));
    let snapshot = collect_entries(root.path().to_path_buf(), entries, ReadOptions::default());

    assert!(snapshot.entries.is_empty());
    assert_eq!(snapshot.warnings.len(), 1);
    assert_eq!(snapshot.warnings[0].path, root.path());
    assert_eq!(snapshot.warnings[0].operation, WarningOperation::Enumerate);
    assert_eq!(
        snapshot.warnings[0].error_kind,
        io::ErrorKind::PermissionDenied
    );
}

#[cfg(unix)]
#[test]
fn permissions_owner_group_and_access_time_come_from_real_metadata() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let root = tempfile::tempdir().expect("create directory fixture");
    let tool = root.path().join("setuid-tool");
    fs::write(&tool, b"#!/bin/sh\n").expect("create executable fixture");
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o4755)).expect("set setuid permissions");
    let shared = root.path().join("shared");
    fs::create_dir(&shared).expect("create shared directory");
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o1777))
        .expect("set sticky permissions");
    let setgid = root.path().join("setgid-folder");
    fs::create_dir(&setgid).expect("create setgid directory");
    fs::set_permissions(&setgid, fs::Permissions::from_mode(0o2750))
        .expect("set setgid permissions");
    let setuid_without_execute = root.path().join("setuid-noexec");
    fs::write(&setuid_without_execute, b"data").expect("create fixture");
    fs::set_permissions(&setuid_without_execute, fs::Permissions::from_mode(0o4644))
        .expect("set setuid permissions without execute");
    let locked = root.path().join("readonly.txt");
    fs::write(&locked, b"data").expect("create read-only fixture");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o444))
        .expect("set read-only permissions");

    let snapshot = read_directory(root.path(), ReadOptions::default()).expect("read fixture");
    let tool = named(&snapshot, "setuid-tool");
    let permissions = tool.details.permissions.expect("permissions are available");
    assert_eq!(permissions.symbols(), "rwsr-xr-x");
    assert_eq!(permissions.octal(), "4755");
    assert!(permissions.setuid && !permissions.setgid && !permissions.sticky);
    assert_eq!(tool.details.read_only, Some(false));

    let shared = named(&snapshot, "shared");
    let permissions = shared
        .details
        .permissions
        .expect("permissions are available");
    assert_eq!(permissions.symbols(), "rwxrwxrwt");
    assert!(permissions.sticky);

    let setgid_permissions = named(&snapshot, "setgid-folder")
        .details
        .permissions
        .expect("permissions are available");
    assert_eq!(setgid_permissions.symbols(), "rwxr-s---");
    assert_eq!(setgid_permissions.octal(), "2750");
    assert!(setgid_permissions.setgid);

    let no_execute = named(&snapshot, "setuid-noexec")
        .details
        .permissions
        .expect("permissions are available");
    assert_eq!(no_execute.symbols(), "rwSr--r--");
    assert_eq!(no_execute.octal(), "4644");

    assert_eq!(
        named(&snapshot, "readonly.txt").details.read_only,
        Some(true)
    );

    let metadata = fs::symlink_metadata(&tool.path).expect("stat fixture");
    let owner = tool.details.owner.as_ref().expect("owner is available");
    assert_eq!(owner.uid, metadata.uid());
    let expected_owner = owner.name.clone().unwrap_or_else(|| owner.uid.to_string());
    assert_eq!(owner.label().to_string(), expected_owner);
    let group = tool.details.group.as_ref().expect("group is available");
    assert_eq!(group.gid, metadata.gid());
    let expected_group = group.name.clone().unwrap_or_else(|| group.gid.to_string());
    assert_eq!(group.label().to_string(), expected_group);
    assert!(tool.details.accessed.is_some());
}

#[cfg(unix)]
#[test]
fn owner_flag_marks_entries_that_belong_to_the_current_user() {
    let root = tempfile::tempdir().expect("create directory fixture");
    fs::write(root.path().join("mine"), b"data").expect("create fixture");

    let snapshot = read_directory(root.path(), ReadOptions::default()).expect("read fixture");
    let owner = snapshot.entries[0]
        .details
        .owner
        .as_ref()
        .expect("owner is available");

    assert!(
        owner.is_current_user,
        "the fixture belongs to the current user"
    );
}

#[test]
fn content_detection_wins_over_the_file_name() {
    let root = tempfile::tempdir().expect("create directory fixture");
    fs::write(root.path().join("photo.jpg"), PNG_1X1).expect("create PNG with a JPEG name");

    let snapshot = read_directory(root.path(), ReadOptions::default()).expect("read fixture");
    let entry = &snapshot.entries[0];
    let content = entry.details.content.as_ref().expect("content is detected");

    assert_eq!(entry.name, OsString::from("photo.jpg"));
    assert_eq!(content.category, ContentCategory::Image);
    assert_eq!(content.format.as_deref(), Some("PNG"));
    assert_eq!(content.mime.as_deref(), Some("image/png"));
}

#[test]
fn source_and_plain_text_are_named_from_the_extension() {
    let root = tempfile::tempdir().expect("create directory fixture");
    fs::write(root.path().join("lib.rs"), b"fn main() {}\n").expect("create source fixture");
    fs::write(root.path().join("notes.txt"), b"plain words\n").expect("create text fixture");

    let snapshot = read_directory(root.path(), ReadOptions::default()).expect("read fixture");
    let source = named(&snapshot, "lib.rs")
        .details
        .content
        .as_ref()
        .expect("content is detected");
    assert_eq!(source.category, ContentCategory::Code);
    assert_eq!(source.format.as_deref(), Some("Rust"));

    let text = named(&snapshot, "notes.txt")
        .details
        .content
        .as_ref()
        .expect("content is detected");
    assert_eq!(text.category, ContentCategory::Text);
    assert_eq!(text.format, None);
}

#[test]
fn empty_content_falls_back_to_the_extension() {
    let root = tempfile::tempdir().expect("create directory fixture");
    fs::write(root.path().join("empty.png"), b"").expect("create empty file");

    let snapshot = read_directory(root.path(), ReadOptions::default()).expect("read fixture");
    let content = named(&snapshot, "empty.png")
        .details
        .content
        .as_ref()
        .expect("the extension still names the type");

    assert_eq!(content.category, ContentCategory::Image);
    assert_eq!(content.format.as_deref(), Some("PNG"));
}

#[cfg(unix)]
#[test]
fn directory_and_dangling_symlinks_are_not_followed_during_listing() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("create directory fixture");
    let target = tempfile::tempdir().expect("create linked directory");
    fs::write(target.path().join("nested"), b"outside listing").expect("create linked content");
    symlink(target.path(), root.path().join("directory-link")).expect("create directory link");
    symlink(
        root.path().join("absent"),
        root.path().join("dangling-link"),
    )
    .expect("create dangling link");
    symlink("loop", root.path().join("loop")).expect("create self-referential link");

    let snapshot = read_directory(root.path(), ReadOptions::default()).expect("read links");

    assert_eq!(snapshot.entries.len(), 3);
    assert!(
        snapshot
            .entries
            .iter()
            .all(|entry| { entry.kind == EntryKind::SymbolicLink && entry.size.is_none() })
    );
    assert!(snapshot.warnings.is_empty());

    let directory_link = named(&snapshot, "directory-link")
        .details
        .target
        .as_ref()
        .expect("target is classified");
    assert_eq!(directory_link.status, LinkStatus::Resolved);
    assert_eq!(directory_link.kind, EntryKind::Directory);
    assert!(directory_link.content.is_none());

    let dangling = named(&snapshot, "dangling-link")
        .details
        .target
        .as_ref()
        .expect("target is classified");
    assert_eq!(dangling.status, LinkStatus::Broken);
    assert_eq!(dangling.kind, EntryKind::Unknown);
    assert!(dangling.content.is_none());

    // A loop is not a dangling link: the target exists in principle but cannot
    // be inspected, so it must not be reported as either present or broken.
    let looping = named(&snapshot, "loop")
        .details
        .target
        .as_ref()
        .expect("target is classified");
    assert_eq!(looping.status, LinkStatus::Unreachable);
    assert_eq!(looping.kind, EntryKind::Unknown);

    // The link itself keeps its own ownership and mode instead of the target's.
    assert!(
        named(&snapshot, "dangling-link")
            .details
            .permissions
            .is_some()
    );
}

#[cfg(unix)]
#[test]
fn symlink_targets_are_classified_by_their_own_content() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("create directory fixture");
    let target = root.path().join("real-image");
    fs::write(&target, PNG_1X1).expect("create PNG without an extension");
    symlink(&target, root.path().join("shortcut.jpg")).expect("create file link");

    let snapshot = read_directory(root.path(), ReadOptions::default()).expect("read links");
    let entry = named(&snapshot, "shortcut.jpg");
    assert_eq!(entry.kind, EntryKind::SymbolicLink);

    let target = entry.details.target.as_ref().expect("target is classified");
    assert_eq!(target.status, LinkStatus::Resolved);
    assert_eq!(target.kind, EntryKind::File);
    let content = target.content.as_ref().expect("target content is detected");
    assert_eq!(content.category, ContentCategory::Image);
    assert_eq!(content.format.as_deref(), Some("PNG"));
}

#[cfg(unix)]
#[test]
fn non_utf8_names_remain_usable_as_paths() {
    use std::os::unix::ffi::OsStringExt;

    let root = tempfile::tempdir().expect("create directory fixture");
    let name = OsString::from_vec(vec![b'f', 0xff, b'e']);
    let path = root.path().join(&name);
    fs::write(&path, b"exact original bytes").expect("create non-UTF-8 filename");

    let snapshot =
        read_directory(root.path(), ReadOptions::default()).expect("read non-UTF-8 entry");
    let entry = &snapshot.entries[0];

    assert_eq!(entry.name, name);
    assert_eq!(entry.path, path);
    assert_eq!(
        fs::read(&entry.path).expect("open original path"),
        b"exact original bytes"
    );
    assert!(!entry.display_name.is_empty());
    assert_eq!(
        entry
            .details
            .content
            .as_ref()
            .map(|content| content.category),
        Some(ContentCategory::Text)
    );
}
