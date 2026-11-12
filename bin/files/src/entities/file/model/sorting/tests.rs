use super::super::columns::{Column, Layout};
use super::{Direction, Key, Sort};
use crate::entities::file::directory::{Entry, EntryDetails, EntryKind};
use std::{ffi::OsString, path::PathBuf};

fn entry(name: impl Into<OsString>, path: &str) -> Entry {
    Entry::new(
        PathBuf::from(path),
        name.into(),
        EntryKind::File,
        Some(7),
        None,
        EntryDetails::default(),
    )
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

#[test]
fn legacy_sort_and_layout_deserialize_into_canonical_kebab_case() {
    let name_sort: Sort = serde_json::from_str(r#"{"key":"Name","direction":"Ascending"}"#)
        .expect("deserialize legacy Name sort");
    assert_eq!(
        name_sort,
        Sort {
            key: Key::Name,
            direction: Direction::Ascending,
        }
    );
    assert_eq!(
        serde_json::to_string(&name_sort).expect("serialize canonical Name sort"),
        r#"{"key":"name","direction":"Ascending"}"#
    );

    let detailed_sort: Sort =
        serde_json::from_str(r#"{"key":"DetailedType","direction":"Descending"}"#)
            .expect("deserialize legacy DetailedType sort");
    assert_eq!(
        detailed_sort,
        Sort {
            key: Key::DetailedType,
            direction: Direction::Descending,
        }
    );
    assert_eq!(
        serde_json::to_string(&detailed_sort).expect("serialize canonical DetailedType sort"),
        r#"{"key":"detailed-type","direction":"Descending"}"#
    );

    let snake_sort: Sort =
        serde_json::from_str(r#"{"key":"detailed_type","direction":"descending"}"#)
            .expect("deserialize snake_case sort aliases");
    assert_eq!(snake_sort, detailed_sort);

    let layout: Layout =
        serde_json::from_str(r#"["Modified","DetailedType","UnknownColumn","Permissions"]"#)
            .expect("deserialize legacy PascalCase layout");
    assert_eq!(
        layout.columns(),
        &[
            Column::Name,
            Column::Modified,
            Column::DetailedType,
            Column::Permissions,
        ]
    );
    assert_eq!(
        serde_json::to_string(&layout).expect("serialize canonical layout"),
        r#"["name","modified","detailed-type","permissions"]"#
    );
}
