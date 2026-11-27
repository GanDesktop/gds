use std::{ffi::OsString, path::PathBuf};

use super::{
    EnrichQueue, EnrichmentNeed, Entry, directory, enrichment_need_of, viewport_entries_from,
};
use crate::{
    entities::{
        file::{ViewMode, columns::Column, sorting},
        location::{Location, places::Places},
    },
    pages::workspace::{
        projection::geometry::Grid,
        state::model::{Row, Tab},
    },
};

fn unprobed_dir_snapshot(count: usize) -> directory::DirectorySnapshot {
    let entries: Vec<Entry> = (0..count)
        .map(|index| {
            let name = format!("dir-{index:04}");
            Entry::new(
                PathBuf::from(format!("/tmp/{name}")),
                OsString::from(&name),
                directory::EntryKind::Directory,
                None,
                None,
                directory::EntryDetails::default(),
            )
        })
        .collect();
    directory::DirectorySnapshot {
        path: PathBuf::from("/tmp"),
        entries: entries.into(),
        warnings: Vec::new(),
        options: directory::ReadOptions::default(),
    }
}

#[test]
fn prioritize_viewport_sheds_transient_scroll_items_under_bounded_budget() {
    let snapshot = unprobed_dir_snapshot(300);
    let need = EnrichmentNeed {
        children: true,
        budget: directory::ProbeBudget::Bounded(0),
        ..Default::default()
    };
    let custom_limit = 64usize;
    let mut queue = EnrichQueue::new(
        &snapshot,
        &[],
        need,
        custom_limit,
        directory::DEFAULT_BATCH_SIZE,
    );
    assert!(queue.is_empty());

    // Simulate rapid scrolling across 10 viewports (200 entries total).
    for step in 0..10 {
        let start = step * 20;
        queue.prioritize_viewport(&snapshot.entries[start..start + 20], need);
    }

    let mut popped = Vec::new();
    while let Some(batch) = queue.pop_batch() {
        assert!(batch.is_viewport);
        popped.extend(batch.entries);
    }
    assert_eq!(popped.len(), custom_limit);
    // Most recent viewport (180..200) is popped first; earliest scrolled-past items (0..20) were shed.
    assert_eq!(popped[0].path, snapshot.entries[180].path);
    assert!(
        !popped
            .iter()
            .any(|entry| entry.path == snapshot.entries[0].path)
    );

    // Revisiting an earlier shed viewport later can still enqueue and probe it cleanly.
    queue.prioritize_viewport(&snapshot.entries[0..10], need);
    let Some(revisited) = queue.pop_batch() else {
        panic!("expected revisited viewport batch");
    };
    assert_eq!(revisited.entries.len(), 10);
    assert_eq!(revisited.entries[0].path, snapshot.entries[0].path);
}

#[test]
fn prioritize_viewport_retains_overflow_in_backlog_under_full_budget() {
    let snapshot = unprobed_dir_snapshot(200);
    let need = EnrichmentNeed {
        children: true,
        budget: directory::ProbeBudget::Full,
        ..Default::default()
    };
    let mut queue = EnrichQueue::new(
        &snapshot,
        &[],
        need,
        directory::DEFAULT_PRIORITY_LIMIT,
        directory::DEFAULT_BATCH_SIZE,
    );

    for step in 0..10 {
        let start = step * 20;
        queue.prioritize_viewport(&snapshot.entries[start..start + 20], need);
    }

    let mut popped = Vec::new();
    while let Some(batch) = queue.pop_batch() {
        popped.extend(batch.entries);
    }
    // Every unique entry is dispatched exactly once under Full budget.
    assert_eq!(popped.len(), 200);
    assert_eq!(popped[0].path, snapshot.entries[180].path);
}

#[test]
fn enrichment_need_switches_to_full_budget_for_metadata_sorts() {
    let places = Places::default();
    let mut tab = Tab::new(
        1,
        Location::from(PathBuf::from("/tmp").as_path()),
        &places,
        false,
    );
    tab.mode = ViewMode::List;
    tab.columns.show(Column::Size);
    tab.columns.show(Column::Type);
    tab.sort = sorting::Sort {
        key: sorting::Key::Name,
        direction: sorting::Direction::Ascending,
    };

    let (need, sort_dep) = enrichment_need_of(&tab, 50, true, false, false);
    assert!(need.children);
    assert!(need.content);
    assert!(need.symlinks);
    assert_eq!(need.budget, directory::ProbeBudget::Bounded(50));
    assert!(!sort_dep);

    let (no_probe_need, _) = enrichment_need_of(&tab, 50, false, false, false);
    assert!(no_probe_need.children);
    assert!(!no_probe_need.content);
    assert!(no_probe_need.symlinks);

    tab.sort.key = sorting::Key::DetailedType;
    let (sort_need, sort_dep) = enrichment_need_of(&tab, 50, true, false, false);
    assert_eq!(sort_need.budget, directory::ProbeBudget::Full);
    assert!(sort_dep);
}

#[test]
fn viewport_entries_from_slices_list_and_grid_rows() {
    let places = Places::default();
    let mut tab = Tab::new(
        1,
        Location::from(PathBuf::from("/tmp").as_path()),
        &places,
        false,
    );
    let snap = unprobed_dir_snapshot(40);
    tab.rows = snap.entries.iter().cloned().map(Row::File).collect();
    let grid = Grid::new(800.0, 1.0);

    tab.mode = ViewMode::List;
    let list_slice = viewport_entries_from(&tab, &grid, 1.0, 40.0, 120.0, 80.0);
    assert_eq!(list_slice.len(), 3);
    assert_eq!(list_slice[0].path, snap.entries[2].path);

    tab.mode = ViewMode::Grid;
    let grid_slice = viewport_entries_from(&tab, &grid, 1.0, 40.0, grid.row_height, 0.0);
    assert!(!grid_slice.is_empty());
    assert_eq!(grid_slice[0].path, snap.entries[0].path);
}
