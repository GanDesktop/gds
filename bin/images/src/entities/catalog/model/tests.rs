use std::{
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use super::{CatalogEntry, CatalogEntryMeta, CatalogState};

fn sample_catalog(current_index: Option<usize>) -> CatalogState {
    CatalogState {
        directory: Some(PathBuf::from("/tmp")),
        entries: vec![
            CatalogEntry::new(
                PathBuf::from("/tmp/a.png"),
                CatalogEntryMeta::new(Path::new("/tmp/a.png"), Some(100), None),
            ),
            CatalogEntry::new(
                PathBuf::from("/tmp/b.png"),
                CatalogEntryMeta::new(Path::new("/tmp/b.png"), Some(200), None),
            ),
            CatalogEntry::new(
                PathBuf::from("/tmp/c.png"),
                CatalogEntryMeta::new(Path::new("/tmp/c.png"), Some(300), None),
            ),
            CatalogEntry::new(
                PathBuf::from("/tmp/d.png"),
                CatalogEntryMeta::new(Path::new("/tmp/d.png"), Some(400), None),
            ),
        ],
        current_index,
    }
}

#[test]
fn visible_window_never_pads_empty_slots_and_clamps_bounds() {
    let at_first = sample_catalog(Some(0));
    let (start, items) = at_first.visible_window(7);
    assert_eq!(start, 0);
    assert_eq!(items.len(), 4);

    let at_first_small = at_first.visible_window(3);
    assert_eq!(at_first_small.0, 0);
    let paths: Vec<_> = at_first_small.1.iter().map(|e| e.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("/tmp/a.png"),
            PathBuf::from("/tmp/b.png"),
            PathBuf::from("/tmp/c.png"),
        ]
    );

    let at_middle = sample_catalog(Some(2));
    let (start, items) = at_middle.visible_window(3);
    assert_eq!(start, 1);
    let paths: Vec<_> = items.iter().map(|e| e.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("/tmp/b.png"),
            PathBuf::from("/tmp/c.png"),
            PathBuf::from("/tmp/d.png"),
        ]
    );

    let at_last = sample_catalog(Some(3));
    let (start, items) = at_last.visible_window(3);
    assert_eq!(start, 1);
    assert_eq!(items.len(), 3);
    assert_eq!(items[2].path, PathBuf::from("/tmp/d.png"));

    assert_eq!(CatalogState::default().visible_window(5), (0, &[][..]));
    assert_eq!(at_first.visible_window(0), (0, &[][..]));
}

#[test]
fn select_rename_and_update_entry_meta_keep_natural_order_and_cached_strings() {
    let mut catalog = sample_catalog(Some(0));
    assert!(catalog.select_path(Path::new("/tmp/c.png")));
    assert_eq!(catalog.current_index, Some(2));
    let meta = catalog
        .meta_for_path(Path::new("/tmp/c.png"))
        .expect("meta for c.png");
    assert_eq!(meta.bytes, Some(300));
    assert_eq!(meta.display_name.as_ref(), "c.png");
    assert_eq!(meta.subtitle.as_ref(), "PNG · 300 B");
    assert!(!catalog.select_path(Path::new("/tmp/missing.png")));

    let renamed = PathBuf::from("/tmp/aa.png");
    catalog.rename_path(
        Path::new("/tmp/c.png"),
        renamed.clone(),
        CatalogEntryMeta::new(&renamed, Some(2048), None),
    );
    assert_eq!(catalog.current_index, Some(1));
    assert_eq!(catalog.entries[1].path, renamed);
    let renamed_meta = catalog.meta_at(1).expect("renamed meta");
    assert_eq!(renamed_meta.bytes, Some(2048));
    assert_eq!(renamed_meta.subtitle.as_ref(), "PNG · 2.0 KiB");

    let stamped = UNIX_EPOCH + Duration::from_secs(42);
    assert!(catalog.update_entry_meta(Path::new("/tmp/aa.png"), 3 * 1024 * 1024, Some(stamped)));
    let updated_meta = catalog.meta_at(1).expect("updated meta");
    assert_eq!(updated_meta.bytes, Some(3 * 1024 * 1024));
    assert_eq!(updated_meta.modified, Some(stamped));
    assert_eq!(updated_meta.subtitle.as_ref(), "PNG · 3.0 MiB");
}

#[test]
fn rename_and_remove_preserve_active_selection_when_modifying_other_entries() {
    // Navigating to `c.png` (index 2) while renaming `a.png` (index 0) after it (`z.png`) must keep `c.png` active.
    let mut catalog = sample_catalog(Some(2));
    let moved_after = PathBuf::from("/tmp/z.png");
    catalog.rename_path(
        Path::new("/tmp/a.png"),
        moved_after.clone(),
        CatalogEntryMeta::new(&moved_after, Some(50), None),
    );
    assert_eq!(catalog.current_path(), Some(&PathBuf::from("/tmp/c.png")));
    assert_eq!(catalog.current_index, Some(1));

    // Navigating to `b.png` (index 0 now: [b, c, d, z]) while renaming `d.png` (index 2) before it (`0.png`) keeps `b.png`.
    catalog.select_path(Path::new("/tmp/b.png"));
    assert_eq!(catalog.current_index, Some(0));
    let moved_before = PathBuf::from("/tmp/0.png");
    catalog.rename_path(
        Path::new("/tmp/d.png"),
        moved_before.clone(),
        CatalogEntryMeta::new(&moved_before, Some(50), None),
    );
    assert_eq!(catalog.current_path(), Some(&PathBuf::from("/tmp/b.png")));
    assert_eq!(catalog.current_index, Some(1));

    // Removing an entry before the active item decrements `current_index` without switching images.
    let neighbor = catalog.remove_path(Path::new("/tmp/0.png"));
    assert_eq!(neighbor, Some(PathBuf::from("/tmp/b.png")));
    assert_eq!(catalog.current_path(), Some(&PathBuf::from("/tmp/b.png")));
    assert_eq!(catalog.current_index, Some(0));

    // Removing an entry after the active item leaves `current_index` untouched.
    catalog.remove_path(Path::new("/tmp/z.png"));
    assert_eq!(catalog.current_path(), Some(&PathBuf::from("/tmp/b.png")));
    assert_eq!(catalog.current_index, Some(0));

    // Removing the currently active entry advances `current_index` to the returned neighbor.
    let next = catalog.remove_path(Path::new("/tmp/b.png"));
    assert_eq!(next, Some(PathBuf::from("/tmp/c.png")));
    assert_eq!(catalog.current_path(), Some(&PathBuf::from("/tmp/c.png")));
    assert_eq!(catalog.current_index, Some(0));

    // When no item was selected, mutating entries leaves `current_index == None`.
    let mut unselected = sample_catalog(None);
    let renamed = PathBuf::from("/tmp/zz.png");
    unselected.rename_path(
        Path::new("/tmp/a.png"),
        renamed.clone(),
        CatalogEntryMeta::new(&renamed, Some(10), None),
    );
    unselected.remove_path(Path::new("/tmp/b.png"));
    assert_eq!(unselected.current_index, None);
}
