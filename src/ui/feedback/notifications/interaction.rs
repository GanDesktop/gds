//! Keyboard interaction stays with the shared notification host.

use super::*;
use crate::ui::actions::Cancel;
use gpui_kit::{KeyDownEvent, prelude::*};

pub fn is_focused(window: &Window, cx: &App) -> bool {
    overlay_for(window, cx).is_some_and(|view| view.read(cx).focus.contains_focused(window, cx))
}

pub fn focus(window: &mut Window, cx: &mut App) {
    if let Some(view) = overlay_for(window, cx) {
        view.update(cx, |view, cx| view.focus_current(window, cx));
    }
}

pub fn close_current(window: &mut Window, cx: &mut App) {
    if let Some(view) = overlay_for(window, cx) {
        view.update(cx, |view, cx| view.close_current(window, cx));
    }
}

pub fn close_all(window: &mut Window, cx: &mut App) {
    if let Some(view) = overlay_for(window, cx) {
        view.update(cx, |view, cx| view.close_all(window, cx));
    }
}

impl Overlay {
    pub(super) fn available(&self, cx: &App) -> Vec<Entity<Notification>> {
        self.list
            .read(cx)
            .notifications()
            .into_iter()
            .filter(|note| !self.closing.contains(&note.entity_id()))
            .collect()
    }

    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if self.focus.contains_focused(window, cx)
            && let Some(focus) = self.return_focus.take()
        {
            focus.focus(window, cx);
        }
    }

    pub fn focus_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus.contains_focused(window, cx) {
            self.restore_focus(window, cx);
        } else {
            let notes = self.available(cx);
            let Some(note) = notes.last() else {
                return;
            };
            self.selected_index = notes.len() - 1;
            self.selected = Some(note.entity_id());
            self.return_focus = window.focused(cx);
            self.focus.focus(window, cx);
            self.scroll.scroll_to_item(self.selected_index);
        }
        cx.notify();
    }

    pub(super) fn sync_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mounted = self.list.read(cx).notifications();
        self.closing
            .retain(|id| mounted.iter().any(|note| note.entity_id() == *id));
        let notes = self.available(cx);
        if notes.is_empty() {
            self.selected = None;
            self.restore_focus(window, cx);
        } else if let Some(index) = notes
            .iter()
            .position(|note| Some(note.entity_id()) == self.selected)
        {
            self.selected_index = index;
        } else {
            self.selected_index = self.selected_index.min(notes.len() - 1);
            self.selected = Some(notes[self.selected_index].entity_id());
        }
    }

    pub fn close_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let notes = self.available(cx);
        let current = if self.focus.contains_focused(window, cx) {
            notes
                .iter()
                .find(|note| Some(note.entity_id()) == self.selected)
                .or(notes.last())
        } else {
            notes.last()
        };
        let Some(note) = current else {
            return;
        };
        self.closing.insert(note.entity_id());
        note.update(cx, |note, cx| note.dismiss(window, cx));
        self.sync_selection(window, cx);
        self.scroll.scroll_to_item(self.selected_index);
        cx.notify();
    }

    pub fn close_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.closing
            .extend(self.available(cx).iter().map(|note| note.entity_id()));
        self.restore_focus(window, cx);
        self.selected = None;
        self.list.update(cx, |list, cx| list.clear(window, cx));
        cx.notify();
    }

    pub(super) fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let notes = self.available(cx);
        if notes.is_empty() {
            return;
        }
        let current = notes
            .iter()
            .position(|note| Some(note.entity_id()) == self.selected)
            .unwrap_or(notes.len() - 1);
        let next = match event.keystroke.key.as_str() {
            "up" => current.saturating_sub(1),
            "down" => (current + 1).min(notes.len() - 1),
            "home" => 0,
            "end" => notes.len() - 1,
            _ => return,
        };
        self.selected_index = next;
        self.selected = Some(notes[next].entity_id());
        self.scroll.scroll_to_item(next);
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }

    pub(super) fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_focus(window, cx);
        cx.stop_propagation();
        cx.notify();
    }
}
