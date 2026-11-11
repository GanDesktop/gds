//! File-tab cycling, ordering and direct ordinal shortcuts.
mod reorder;

use super::*;
use gds::ui::DialogActions;
use gds_uim::el;
use gpui_kit::{assets::IconName, component::Icon};

pub(in crate::pages::workspace) struct State {
    pub items: Vec<OpenTab>,
    pub active_id: u64,
    pub next_id: u64,
    /// The hovered row reveals its close control alongside the held Ctrl group.
    pub hovered: Option<u64>,
}

impl State {
    fn move_tab(&mut self, source: u64, target: u64, after: bool) -> bool {
        let Some(from) = self.items.iter().position(|tab| tab.id == source) else {
            return false;
        };
        let Some(to) = self.items.iter().position(|tab| tab.id == target) else {
            return false;
        };
        if source == target || self.items[from].pinned != self.items[to].pinned {
            return false;
        }
        let insertion = to + usize::from(after);
        let destination = insertion - usize::from(from < insertion);
        if from == destination {
            return false;
        }
        let tab = self.items.remove(from);
        self.items.insert(destination, tab);
        true
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            active_id: 0,
            next_id: 1,
            hovered: None,
        }
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn select_ordinal_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pages.is_empty()
            || window.has_active_dialog(cx)
            || self.terminal_focused(window, cx)
        {
            return;
        }
        if let Some(id) = self.tabs.items.get(index).map(|tab| tab.id) {
            self.switch_tab(id, window, cx);
        }
    }

    pub(in crate::pages::workspace) fn show_location(
        &mut self,
        location: Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pages.clear();
        self.navigate_location(self.tabs.active_id, location, cx);
        self.sync_filter_input(window, cx);

        self.window.focus.focus(window, cx);
        cx.notify();
    }

    pub(in crate::pages::workspace) fn location_selected(&self, target: LocationTarget) -> bool {
        self.current_location() == target.location(&self.locations.home)
    }

    /// Toggle a tab row's hover state so its close control follows the pointer.
    pub(in crate::pages::workspace) fn hover_tab(
        id: u64,
    ) -> impl Fn(&mut Self, &bool, &mut Window, &mut Context<Self>) {
        move |this, hovered, _, cx| {
            let next = if *hovered {
                Some(id)
            } else if this.tabs.hovered == Some(id) {
                None
            } else {
                return;
            };
            if this.tabs.hovered != next {
                this.tabs.hovered = next;
                cx.notify();
            }
        }
    }

    /// Clear the Recent list; only the Recent page runs this.
    ///
    /// The Recent cursor names a record that no longer exists afterwards, so it
    /// is dropped with the list. Favorites and pinned folders are untouched.
    pub(in crate::pages::workspace) fn clear_recent(&mut self, cx: &mut Context<Self>) {
        if self.collection() != Some(Collection::Recent) {
            return;
        }
        self.locations.places.clear_recent();
        if let Some(slot) = self.current_tab_mut() {
            slot.navigation.view_mut().set_place(None);
        }
        self.save_places(cx);
        self.sync_workspace_state(cx);
        cx.notify();
    }

    pub(in crate::pages::workspace) fn on_select_tab(
        &mut self,
        action: &SelectTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_ordinal_tab(action.0, window, cx);
    }

    pub(in crate::pages::workspace) fn confirm_close_pinned_tab(
        &self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(tab) = self.tab_slot(id) else {
            return;
        };
        let name = self.tab_title(tab, cx);
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let view = view.clone();
            el! { dialog {
                confirm;
                title: $(i18n::text("unpin-close-title", cx));
                description: $(i18n::t!(@cx, "unpin-close-message", name = name.clone()));
                icon: IconName::Pin.Icon::new;
                footer: $(el! { .DialogActions::default {
                    primary_label: $(i18n::text("unpin-close-confirm", cx));
                    primary_icon: IconName::X;
                    cancel_label: $(i18n::text("operation-cancel", cx));
                } });
                on_ok: $(move |_, window, cx| {
                    if let Some(view) = view.upgrade() {
                        view.update(cx, |view, cx| view.close_tab_approved(id, window, cx));
                    }
                    true
                });
            } }
        });
    }
}
