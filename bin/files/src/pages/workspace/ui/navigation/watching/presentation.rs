use super::*;
use crate::entities::file::{
    columns::{Column, Layout},
    ui::{EntryPresentation, FileIcon},
};

struct RowComparison<'a> {
    grid: bool,
    layout: &'a Layout,
    now: std::time::SystemTime,
}

pub(in crate::pages::workspace) fn changed(
    slot: &OpenTab,
    previous: &[directory::Entry],
    layout: &Layout,
    details: bool,
    cx: &App,
) -> bool {
    let rows = &slot.table_rows;
    if previous.len() != rows.len() {
        return true;
    }
    let grid = slot.view_mode == ViewMode::Grid;
    let scroll = slot.scroll.0.borrow();
    let Some(size) = scroll.last_item_size else {
        return previous != rows.as_ref();
    };
    let height = f32::from(size.item.height).max(1.);
    let top = -f32::from(scroll.base_handle.offset().y);
    let viewport = f32::from(scroll.base_handle.bounds().size.height);
    let columns = if grid { slot.grid.columns } else { 1 };
    let start = (top.max(0.) / height).floor() as usize * columns;
    let end = (((top + viewport).max(0.) / height).ceil() as usize * columns).min(rows.len());
    let comparison = RowComparison {
        grid,
        layout,
        now: ui::time::now(cx),
    };
    for (index, (before, after)) in previous.iter().zip(rows.iter()).enumerate() {
        if before == after {
            continue;
        }
        let selected = slot.navigation.view().selected.contains(&after.path);
        if selected
            && (details
                || EntryPresentation::size_label(before, cx)
                    != EntryPresentation::size_label(after, cx))
        {
            return true;
        }
        if !(start..end).contains(&index) {
            continue;
        }
        if row_changed(before, after, &comparison, cx) {
            return true;
        }
    }
    false
}

fn row_changed(
    before: &directory::Entry,
    after: &directory::Entry,
    comparison: &RowComparison<'_>,
    cx: &App,
) -> bool {
    let format = ui::time::format(cx);
    if before.path != after.path
        || before.display_name != after.display_name
        || before.is_hidden != after.is_hidden
        || FileIcon::for_entry(before) != FileIcon::for_entry(after)
        || FileIcon::color(before, cx) != FileIcon::color(after, cx)
    {
        return true;
    }
    if comparison.grid {
        if before.thumbnail_source() != after.thumbnail_source() {
            return true;
        }
        return false;
    }
    for column in comparison.layout.columns() {
        let changed = match column {
            Column::Name => false,
            Column::Size => {
                EntryPresentation::size_label(before, cx)
                    != EntryPresentation::size_label(after, cx)
            }
            Column::Type => {
                EntryPresentation::type_label(before, cx)
                    != EntryPresentation::type_label(after, cx)
            }
            Column::DetailedType => {
                EntryPresentation::detailed_type(before, cx)
                    != EntryPresentation::detailed_type(after, cx)
            }
            Column::Owner => before.details.owner != after.details.owner,
            Column::Group => before.details.group != after.details.group,
            Column::Permissions => before.details.permissions != after.details.permissions,
            Column::Modified => {
                format.presentation(before.modified, comparison.now)
                    != format.presentation(after.modified, comparison.now)
            }
            Column::Accessed => {
                format.presentation(before.details.accessed, comparison.now)
                    != format.presentation(after.details.accessed, comparison.now)
            }
        };
        if changed {
            return true;
        }
    }
    false
}
