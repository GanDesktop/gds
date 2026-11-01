use std::{
    ffi::OsString,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{Direction, Key, Sort};
use crate::entities::file::directory::{
    ChildCount, ContentCategory, ContentType, Entry, EntryDetails, EntryKind, Group, LinkStatus,
    LinkTarget, Owner, Permissions,
};

fn entry(name: &str, kind: EntryKind, size: Option<u64>, modified: Option<u64>) -> Entry {
    Entry {
        path: PathBuf::from("/fixture").join(name),
        name: name.into(),
        display_name: name.into(),
        kind,
        size,
        modified: modified.map(seconds),
        is_hidden: false,
        details: EntryDetails::default(),
    }
}

fn seconds(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn content(category: ContentCategory, format: &str) -> ContentType {
    ContentType {
        category,
        format: Some(format.into()),
        mime: None,
    }
}

fn order(entries: &[Entry], key: Key, direction: Direction) -> Vec<usize> {
    let mut indices = (0..entries.len()).collect::<Vec<_>>();
    Sort { key, direction }.sort_indices(entries, &mut indices);
    indices
}

#[test]
fn directory_priority_and_missing_metadata_survive_reverse_sort() {
    let entries = vec![
        entry("large", EntryKind::File, Some(20), Some(20)),
        entry("unreadable", EntryKind::File, None, None),
        entry("small", EntryKind::File, Some(10), Some(10)),
        entry("folder", EntryKind::Directory, None, Some(5)),
        entry("unreadable-folder", EntryKind::Directory, None, None),
    ];
    for key in [Key::Size, Key::Modified] {
        let mut indices = vec![0, 1, 2, 3, 4];
        Sort {
            key,
            direction: Direction::Ascending,
        }
        .sort_indices(&entries, &mut indices);
        assert_eq!(indices, [3, 4, 2, 0, 1]);
        Sort {
            key,
            direction: Direction::Descending,
        }
        .sort_indices(&entries, &mut indices);
        assert_eq!(indices, [3, 4, 0, 2, 1]);
    }
}

#[test]
fn size_sort_orders_directories_by_child_count_and_keeps_unknown_counts_last() {
    let mut entries = vec![
        entry("big-folder", EntryKind::Directory, None, None),
        entry("small-folder", EntryKind::Directory, None, None),
        entry("unknown-folder", EntryKind::Directory, None, None),
        entry("large-file", EntryKind::File, Some(4096), None),
        entry("small-file", EntryKind::File, Some(10), None),
    ];
    entries[0].details.children = Some(ChildCount {
        count: 12,
        truncated: false,
    });
    entries[1].details.children = Some(ChildCount {
        count: 2,
        truncated: false,
    });

    // Directories stay first and a folder whose children could not be counted
    // stays after the counted ones, in both directions.
    assert_eq!(
        order(&entries, Key::Size, Direction::Ascending),
        [1, 0, 2, 4, 3]
    );
    assert_eq!(
        order(&entries, Key::Size, Direction::Descending),
        [0, 1, 2, 3, 4]
    );
}

#[test]
fn names_reverse_inside_directory_groups_and_new_keys_start_ascending() {
    let mut entries = vec![
        entry("z-file", EntryKind::File, None, None),
        entry("a-folder", EntryKind::Directory, None, None),
        entry("a-file", EntryKind::File, None, None),
        entry("z-folder", EntryKind::Directory, None, None),
    ];
    for entry in &mut entries {
        entry.display_name = "same display label".into();
    }
    let mut indices = vec![0, 1, 2, 3];
    let sort = Sort::default();
    sort.sort_indices(&entries, &mut indices);
    assert_eq!(indices, [1, 3, 2, 0]);
    let descending = sort.next(Key::Name);
    descending.sort_indices(&entries, &mut indices);
    assert_eq!(indices, [3, 1, 0, 2]);
    assert_eq!(
        descending.next(Key::Owner),
        Sort {
            key: Key::Owner,
            direction: Direction::Ascending
        }
    );
}

#[test]
fn equal_metadata_uses_raw_names_and_paths_with_stable_identical_entries() {
    let mut entries = vec![
        entry("z", EntryKind::File, Some(7), None),
        entry("a", EntryKind::File, Some(7), None),
        entry("a", EntryKind::File, Some(7), None),
        entry("a", EntryKind::File, Some(7), None),
    ];
    entries[1].path = "/z/a".into();
    entries[2].path = "/a/a".into();
    entries[3].path = "/a/a".into();
    for entry in &mut entries {
        entry.display_name = "not an ordering key".into();
    }
    for direction in [Direction::Ascending, Direction::Descending] {
        let mut indices = vec![3, 1, 2, 0];
        Sort {
            key: Key::Size,
            direction,
        }
        .sort_indices(&entries, &mut indices);
        assert_eq!(indices, [3, 2, 1, 0]);
    }
}

#[test]
fn type_sort_groups_content_categories_and_keeps_unknown_types_last() {
    let mut entries = vec![
        entry("recording.mp4", EntryKind::File, None, None),
        entry("notes.txt", EntryKind::File, None, None),
        entry("photo.png", EntryKind::File, None, None),
        entry("unreadable", EntryKind::Unknown, None, None),
        entry("directory", EntryKind::Directory, None, None),
        entry("shortcut", EntryKind::SymbolicLink, None, None),
    ];
    entries[0].details.content = Some(content(ContentCategory::Video, "MPEG-4"));
    entries[1].details.content = Some(content(ContentCategory::Text, "Plain text"));
    entries[2].details.content = Some(content(ContentCategory::Image, "PNG"));
    entries[5].details.target = Some(LinkTarget {
        status: LinkStatus::Resolved,
        kind: EntryKind::File,
        content: Some(content(ContentCategory::Image, "PNG")),
    });

    assert_eq!(
        order(&entries, Key::Type, Direction::Ascending),
        [4, 1, 2, 0, 5, 3]
    );
    assert_eq!(
        order(&entries, Key::Type, Direction::Descending),
        [4, 5, 0, 2, 1, 3]
    );
}

#[test]
fn symlink_type_sort_uses_the_target_category_and_keeps_broken_links_last() {
    let mut entries = vec![
        entry("broken", EntryKind::SymbolicLink, None, None),
        entry("to-image", EntryKind::SymbolicLink, None, None),
        entry("to-folder", EntryKind::SymbolicLink, None, None),
        entry("unreachable", EntryKind::SymbolicLink, None, None),
    ];
    entries[0].details.target = Some(LinkTarget {
        status: LinkStatus::Broken,
        kind: EntryKind::Unknown,
        content: None,
    });
    entries[1].details.target = Some(LinkTarget {
        status: LinkStatus::Resolved,
        kind: EntryKind::File,
        content: Some(content(ContentCategory::Image, "PNG")),
    });
    entries[2].details.target = Some(LinkTarget {
        status: LinkStatus::Resolved,
        kind: EntryKind::Directory,
        content: None,
    });
    entries[3].details.target = Some(LinkTarget {
        status: LinkStatus::Unreachable,
        kind: EntryKind::Unknown,
        content: None,
    });

    assert_eq!(
        order(&entries, Key::Type, Direction::Ascending),
        [2, 1, 0, 3]
    );
}

#[test]
fn owner_group_permission_and_access_sorts_keep_missing_metadata_last() {
    let mut owners = vec![
        entry("bob", EntryKind::File, None, None),
        entry("unknown-owner", EntryKind::File, None, None),
        entry("alice", EntryKind::File, None, None),
    ];
    owners[0].details.owner = Some(Owner {
        uid: 1001,
        name: Some("bob".into()),
        is_current_user: false,
    });
    owners[2].details.owner = Some(Owner {
        uid: 1000,
        name: Some("alice".into()),
        is_current_user: true,
    });
    assert_eq!(order(&owners, Key::Owner, Direction::Ascending), [2, 0, 1]);
    assert_eq!(order(&owners, Key::Owner, Direction::Descending), [0, 2, 1]);

    let mut groups = vec![
        entry("staff", EntryKind::File, None, None),
        entry("unknown-group", EntryKind::File, None, None),
        entry("admins", EntryKind::File, None, None),
    ];
    groups[0].details.group = Some(Group {
        gid: 100,
        name: Some("staff".into()),
    });
    groups[2].details.group = Some(Group {
        gid: 50,
        name: Some("admins".into()),
    });
    assert_eq!(order(&groups, Key::Group, Direction::Ascending), [2, 0, 1]);
    assert_eq!(order(&groups, Key::Group, Direction::Descending), [0, 2, 1]);

    let mut permissions = vec![
        entry("world-readable", EntryKind::File, None, None),
        entry("unknown-mode", EntryKind::File, None, None),
        entry("private", EntryKind::File, None, None),
    ];
    permissions[0].details.permissions = Some(Permissions::from_mode(0o444));
    permissions[2].details.permissions = Some(Permissions::from_mode(0o600));
    assert_eq!(
        order(&permissions, Key::Permissions, Direction::Ascending),
        [0, 2, 1]
    );
    assert_eq!(
        order(&permissions, Key::Permissions, Direction::Descending),
        [2, 0, 1]
    );

    let mut accessed = vec![
        entry("recent", EntryKind::File, None, None),
        entry("unknown-time", EntryKind::File, None, None),
        entry("old", EntryKind::File, None, None),
    ];
    accessed[0].details.accessed = Some(seconds(20));
    accessed[2].details.accessed = Some(seconds(10));
    assert_eq!(
        order(&accessed, Key::Accessed, Direction::Ascending),
        [2, 0, 1]
    );
    assert_eq!(
        order(&accessed, Key::Accessed, Direction::Descending),
        [0, 2, 1]
    );
}

#[test]
fn detailed_type_sorts_by_format_name_and_puts_unnamed_formats_last() {
    let mut entries = vec![
        entry("b.zip", EntryKind::File, None, None),
        entry("a.png", EntryKind::File, None, None),
        entry("c.txt", EntryKind::File, None, None),
        entry("d", EntryKind::File, None, None),
    ];
    entries[0].details.content = Some(content(ContentCategory::Archive, "ZIP"));
    entries[1].details.content = Some(content(ContentCategory::Image, "PNG"));
    entries[2].details.content = Some(ContentType {
        category: ContentCategory::Text,
        format: None,
        mime: Some("text/plain".into()),
    });

    assert_eq!(
        order(&entries, Key::DetailedType, Direction::Ascending),
        [1, 0, 2, 3]
    );
    assert_eq!(
        order(&entries, Key::DetailedType, Direction::Descending),
        [0, 1, 2, 3]
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_names_keep_distinct_sort_and_filesystem_identities() {
    use std::os::unix::ffi::OsStringExt;

    let mut entries = vec![
        entry("placeholder", EntryKind::File, None, None),
        entry("placeholder", EntryKind::File, None, None),
    ];
    entries[0].name = OsString::from_vec(vec![b'a', 0xff]);
    entries[1].name = OsString::from_vec(vec![b'a', 0xfe]);
    for entry in &mut entries {
        entry.path = PathBuf::from("/fixture").join(&entry.name);
        entry.display_name = entry.name.to_string_lossy().into_owned();
    }
    assert_eq!(entries[0].display_name, entries[1].display_name);
    let paths = entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let mut indices = vec![0, 1];
    Sort::default().sort_indices(&entries, &mut indices);
    assert_eq!(indices, [1, 0]);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>(),
        paths
    );
}
