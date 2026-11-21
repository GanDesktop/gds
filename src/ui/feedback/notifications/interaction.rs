use zgui::prelude::*;
use zgui::runtime::try_use_clipboard;

use super::NotificationQueue;

/// Toggles keyboard focus into or out of the notification stack (`Alt+N`, `shortcuts.md` §1):
/// focuses the newest/selected notification when unfocused, and restores the prior focus target
/// when pressed again while focused.
pub fn focus() {
    focus_in(NotificationQueue::active());
}

pub(crate) fn focus_in(queue: NotificationQueue) {
    if queue.focused().get_untracked() {
        queue.clear_focus(true);
        return;
    }
    let items = queue.items().get_untracked();
    if items.is_empty() {
        return;
    }
    queue.capture_previous_focus();
    let target_id = queue
        .selected_id()
        .get_untracked()
        .filter(|id| items.iter().any(|item| item.id == *id))
        .unwrap_or(items[0].id);
    queue.focus_notification(target_id);
}

/// Dismisses the currently focused notification, or the newest notification when unfocused (`Alt+X`).
pub fn close_current() {
    close_current_in(NotificationQueue::active());
}

pub(crate) fn close_current_in(queue: NotificationQueue) {
    let items = queue.items().get_untracked();
    let target = if queue.focused().get_untracked() {
        queue
            .selected_id()
            .get_untracked()
            .filter(|id| items.iter().any(|item| item.id == *id))
            .or_else(|| items.first().map(|item| item.id))
    } else {
        items.first().map(|item| item.id)
    };
    if let Some(id) = target {
        queue.dismiss(id);
    }
}

/// Dismisses every active notification and restores any captured prior window focus (`Alt+Shift+X`).
pub fn close_all() {
    close_all_in(NotificationQueue::active());
}

pub(crate) fn close_all_in(queue: NotificationQueue) {
    queue.dismiss_all();
}

/// Returns `true` when keyboard focus currently sits inside the notification stack.
#[must_use]
pub fn is_focused() -> bool {
    NotificationQueue::active()
        .focused()
        .try_get_untracked()
        .unwrap_or(false)
}

#[cfg(test)]
pub(crate) fn select_next() {
    select_next_in(NotificationQueue::active());
}

pub(crate) fn select_next_in(queue: NotificationQueue) {
    move_selection(queue, 1);
}

pub(crate) fn select_prev_in(queue: NotificationQueue) {
    move_selection(queue, -1);
}

pub(crate) fn select_first_in(queue: NotificationQueue) {
    if let Some(first) = queue.items().get_untracked().first() {
        queue.focus_notification(first.id);
    }
}

pub(crate) fn select_last_in(queue: NotificationQueue) {
    if let Some(last) = queue.items().get_untracked().last() {
        queue.focus_notification(last.id);
    }
}

pub(crate) fn copy_selected_in(queue: NotificationQueue) {
    let items = queue.items().get_untracked();
    let target = queue
        .selected_id()
        .get_untracked()
        .and_then(|id| items.iter().find(|item| item.id == id))
        .or_else(|| items.first());
    if let Some(entry) = target
        && let Some(clipboard) = try_use_clipboard()
    {
        clipboard.set_text(ClipboardKind::Standard, entry.report.copy_text());
    }
}

fn move_selection(queue: NotificationQueue, delta: isize) {
    let items = queue.items().get_untracked();
    if items.is_empty() {
        return;
    }
    let current_index = queue
        .selected_id()
        .get_untracked()
        .and_then(|id| items.iter().position(|item| item.id == id))
        .unwrap_or(0);
    let len = items.len();
    let next_index = if delta >= 0 {
        (current_index + 1).min(len - 1)
    } else {
        current_index.saturating_sub(1)
    };
    queue.focus_notification(items[next_index].id);
}
