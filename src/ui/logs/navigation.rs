//! Keyboard list-navigation arithmetic for visible log entries.

use crate::service::logs::Entry;

/// Relative keyboard navigation step within the visible filtered log list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationStep {
    Previous,
    Next,
    PageUp,
    PageDown,
}

#[must_use]
pub(crate) fn step_index(
    current: Option<usize>,
    len: usize,
    step: NavigationStep,
    page_rows: usize,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let last = len - 1;
    let jump = page_rows.max(1);
    Some(match (current, step) {
        (None, NavigationStep::Previous | NavigationStep::PageUp) => last,
        (None, NavigationStep::Next | NavigationStep::PageDown) => 0,
        (Some(index), NavigationStep::Previous) => index.saturating_sub(1),
        (Some(index), NavigationStep::Next) => (index + 1).min(last),
        (Some(index), NavigationStep::PageUp) => index.saturating_sub(jump),
        (Some(index), NavigationStep::PageDown) => (index + jump).min(last),
    })
}

#[must_use]
pub(crate) fn step_selection(
    visible: &[usize],
    entries: &[Entry],
    selected: Option<u64>,
    step: NavigationStep,
    page_rows: usize,
) -> Option<(usize, u64)> {
    let current = selected.and_then(|id| {
        visible
            .iter()
            .position(|&entry_idx| entries.get(entry_idx).is_some_and(|entry| entry.id == id))
    });
    let next_pos = step_index(current, visible.len(), step, page_rows)?;
    let entry_idx = *visible.get(next_pos)?;
    let entry = entries.get(entry_idx)?;
    Some((next_pos, entry.id))
}
