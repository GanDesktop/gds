use super::{Direction, Key, Sort};
use crate::entities::file::directory::{Entry, EntryDetails, EntryKind};
use std::{ffi::OsString, path::PathBuf};

fn entry(name: impl Into<OsString>, path: &str) -> Entry {
    let name = name.into();
    Entry {
        display_name: name.to_string_lossy().into_owned(),
        name,
        path: PathBuf::from(path),
        kind: EntryKind::File,
        size: Some(7),
        modified: None,
        is_hidden: false,
        details: EntryDetails::default(),
    }
}

// Identical metadata, identical display strings and duplicated entries cannot
// be distinguished by a screenshot; the comparator must retain raw identity.
#[test]
fn equal_metadata_uses_raw_names_and_paths_with_stable_identical_entries() {
    let entries = [
        entry("z", "/z"),
        entry("a", "/z/a"),
        entry("a", "/a/a"),
        entry("a", "/a/a"),
    ];
    let mut ascending = vec![3, 1, 2, 0];
    Sort {
        key: Key::Size,
        direction: Direction::Ascending,
    }
    .sort_indices(&entries, &mut ascending);
    assert_eq!(ascending, [3, 2, 1, 0]);
    let mut descending = vec![3, 1, 2, 0];
    Sort {
        key: Key::Size,
        direction: Direction::Descending,
    }
    .sort_indices(&entries, &mut descending);
    assert_eq!(descending, [3, 2, 1, 0]);
}

#[cfg(unix)]
#[test]
fn non_utf8_names_with_identical_display_labels_remain_distinct() {
    use std::os::unix::ffi::OsStringExt;
    let entries = [
        entry(OsString::from_vec(b"a\xff".to_vec()), "/first"),
        entry(OsString::from_vec(b"a\xfe".to_vec()), "/second"),
    ];
    assert_eq!(entries[0].display_name, entries[1].display_name);
    let mut indices = vec![0, 1];
    Sort::default().sort_indices(&entries, &mut indices);
    assert_eq!(indices, [1, 0]);
}
