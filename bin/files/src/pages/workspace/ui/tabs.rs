//! Keyboard shortcuts select ordinary tabs or navigate to a sidebar location.

use super::*;

pub(super) struct State {
    pub items: Vec<OpenTab>,
    pub active_id: u64,
    pub next_id: u64,
    /// The hovered row reveals its close control alongside the held Ctrl group.
    pub hovered: Option<u64>,
    pub prefix: Option<TabPrefix>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            active_id: 0,
            next_id: 1,
            hovered: None,
            prefix: None,
        }
    }
}

/// An armed `Ctrl+Tab` prefix.
pub(super) struct TabPrefix {
    backwards: bool,
    /// A Tab press is still down; auto-repeat must not re-arm the prefix, and
    /// its eventual release commits the cycle.
    tab_down: bool,
    /// A target key already ran; releasing Tab must not cycle as well.
    consumed: bool,
}

/// A page key behind the tab prefix.
#[derive(Clone, Copy)]
pub(super) enum Target {
    Location(LocationTarget),
    Tab(usize),
}

macro_rules! prefix_handlers {
    ($(($method:ident, $action:ident, $target:expr)),* $(,)?) => {
        $(
            pub(super) fn $method(
                &mut self,
                _: &$action,
                window: &mut Window,
                cx: &mut Context<Self>,
            ) {
                self.apply_tab_prefix_target($target, window, cx);
            }
        )*
    };
}

impl Browser {
    /// Arm the prefix and focus the workspace so target keys do not enter an
    /// editor or trigger its bindings. A modal keeps the page unchanged.
    pub(super) fn arm_tab_prefix(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.open || window.has_active_dialog(cx) {
            return;
        }
        if self.tabs.prefix.is_some() {
            // Auto-repeat while Tab is held: the first press already armed it.
            return;
        }
        self.tabs.prefix = Some(TabPrefix {
            backwards,
            tab_down: true,
            consumed: false,
        });
        self.window.focus.focus(window, cx);
        cx.notify();
    }

    /// Commit the prefix when Tab is released without a target key.
    pub(super) fn on_tab_key_up(
        &mut self,
        event: &KeyUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key != "tab" {
            return;
        }
        if !self.window.focus.is_focused(window) {
            if self.tabs.prefix.take().is_some() {
                cx.notify();
            }
            return;
        }
        let Some(prefix) = self.tabs.prefix.take() else {
            return;
        };
        if !prefix.tab_down {
            return;
        }
        if prefix.consumed || self.settings.open || window.has_active_dialog(cx) {
            cx.notify();
            return;
        }
        self.cycle_tab(prefix.backwards, window, cx);
        cx.notify();
    }

    /// Run one target key and consume the prefix.
    pub(super) fn apply_tab_prefix_target(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(prefix) = self.tabs.prefix.as_mut() else {
            return;
        };
        prefix.consumed = true;
        match target {
            Target::Location(target) => {
                self.show_location(target.location(&self.locations.home), window, cx)
            }
            Target::Tab(index) => {
                if let Some(id) = self.tabs.items.get(index).map(|slot| slot.id) {
                    self.switch_tab(id, window, cx);
                }
            }
        }
    }

    /// Mark an armed prefix as used without cycling on its release.
    ///
    /// Page changes from the pointer keep the prefix state until Tab is
    /// released, so a later release cannot cycle a second time, while its
    /// target bindings stop applying because the prefix was taken.
    pub(super) fn dismiss_tab_prefix(&mut self) {
        if let Some(prefix) = &mut self.tabs.prefix {
            prefix.consumed = true;
        }
    }

    /// Drop the prefix completely, without committing its cycle.
    pub(super) fn reset_tab_prefix(&mut self) {
        self.tabs.prefix = None;
    }

    pub(super) fn show_location(
        &mut self,
        location: Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.open = false;
        self.navigate_location(self.tabs.active_id, location, cx);
        self.sync_filter_input(window, cx);
        self.dismiss_tab_prefix();
        self.window.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn location_selected(&self, target: LocationTarget) -> bool {
        self.current_location() == target.location(&self.locations.home)
    }

    /// Toggle a tab row's hover state so its close control follows the pointer.
    pub(super) fn hover_tab(id: u64) -> impl Fn(&mut Self, &bool, &mut Window, &mut Context<Self>) {
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
    pub(super) fn clear_recent(&mut self, cx: &mut Context<Self>) {
        if self.collection() != Some(Collection::Recent) {
            return;
        }
        self.locations.places.clear_recent();
        if let Some(slot) = self.current_tab_mut() {
            slot.navigation.view_mut().place = None;
        }
        self.save_places(cx);
        cx.notify();
    }

    prefix_handlers!(
        (
            on_tab_prefix_home,
            TabPrefixHome,
            Target::Location(LocationTarget::Home)
        ),
        (
            on_tab_prefix_recent,
            TabPrefixRecent,
            Target::Location(LocationTarget::Recent)
        ),
        (
            on_tab_prefix_favorites,
            TabPrefixFavorites,
            Target::Location(LocationTarget::Favorites)
        ),
        (
            on_tab_prefix_network,
            TabPrefixNetwork,
            Target::Location(LocationTarget::Network)
        ),
        (
            on_tab_prefix_volumes,
            TabPrefixVolumes,
            Target::Location(LocationTarget::Volumes)
        ),
        (
            on_tab_prefix_trash,
            TabPrefixTrash,
            Target::Location(LocationTarget::Trash)
        ),
        (on_tab_prefix_1, TabPrefix1, Target::Tab(0)),
        (on_tab_prefix_2, TabPrefix2, Target::Tab(1)),
        (on_tab_prefix_3, TabPrefix3, Target::Tab(2)),
        (on_tab_prefix_4, TabPrefix4, Target::Tab(3)),
        (on_tab_prefix_5, TabPrefix5, Target::Tab(4)),
        (on_tab_prefix_6, TabPrefix6, Target::Tab(5)),
        (on_tab_prefix_7, TabPrefix7, Target::Tab(6)),
        (on_tab_prefix_8, TabPrefix8, Target::Tab(7)),
        (on_tab_prefix_9, TabPrefix9, Target::Tab(8)),
    );
}

/// The binding staged on a location row.
///
/// The key only runs behind the prefix, so the row's hint shows the path to it
/// (`Tab+H`) while the plain layer is up and just the target (`H`) once the
/// prefix is armed.
pub(super) fn location_shortcut(target: LocationTarget) -> ui::Shortcut {
    let action: Box<dyn Action> = match target {
        LocationTarget::Home => Box::new(TabPrefixHome),
        LocationTarget::Recent => Box::new(TabPrefixRecent),
        LocationTarget::Favorites => Box::new(TabPrefixFavorites),
        LocationTarget::Volumes => Box::new(TabPrefixVolumes),
        LocationTarget::Network => Box::new(TabPrefixNetwork),
        LocationTarget::Trash => Box::new(TabPrefixTrash),
    };
    ui::Shortcut::new(action, Some(TAB_PREFIX_CONTEXT)).via_prefix(Command::NextTab)
}

/// The ordinal binding staged on the `position + 1`-th ordinary tab row.
///
/// The strip is the cycle order, pinned tabs included; rows past the ninth have
/// no key and therefore no hint.
pub(super) fn ordinal_shortcut(position: usize) -> Option<ui::Shortcut> {
    let action: Box<dyn Action> = match position {
        0 => Box::new(TabPrefix1),
        1 => Box::new(TabPrefix2),
        2 => Box::new(TabPrefix3),
        3 => Box::new(TabPrefix4),
        4 => Box::new(TabPrefix5),
        5 => Box::new(TabPrefix6),
        6 => Box::new(TabPrefix7),
        7 => Box::new(TabPrefix8),
        8 => Box::new(TabPrefix9),
        _ => return None,
    };
    Some(ui::Shortcut::new(action, Some(TAB_PREFIX_CONTEXT)).via_prefix(Command::NextTab))
}
