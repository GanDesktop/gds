//! Navigation arrows share the native right-click history menu.

use super::*;
use gds::ui::IconButton;
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::menu::{ContextMenuExt, PopupMenuItem},
    prelude::*,
};

#[cfg(test)]
mod tests;

struct Entry {
    tab: u64,
    index: usize,
    location: Location,
    label: String,
}

impl Browser {
    pub(super) fn history_button(
        &self,
        direction: NavigationDirection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, icon, command, label) = match direction {
            NavigationDirection::Back => ("back", IconName::ArrowLeft, Command::Back, "back"),
            NavigationDirection::Forward => {
                ("forward", IconName::ArrowRight, Command::Forward, "forward")
            }
        };
        let enabled = self.current_tab().is_some_and(|slot| match direction {
            NavigationDirection::Back => slot.navigation.can_go_back(),
            NavigationDirection::Forward => slot.navigation.can_go_forward(),
        });
        let on_click = cx.listener(Self::click_command(command));
        let button = el! {
            id.IconButton::new { icon: icon; label: $i18n::text(label, cx);
                command: command; disabled: $(!enabled); on_click: on_click;
            }
        }
        .into_any_element();
        if !enabled {
            return button;
        }
        let owner = cx.entity().downgrade();
        el! { .div { # format!("history-{id}"); flex; flex_shrink_0; } [button] }
            .context_menu(move |menu, _, cx| {
                let entries = owner
                    .update(cx, |this, cx| this.prepare_history_menu(direction, cx))
                    .unwrap_or_default();
                let mut menu = ui::menu::popup(menu);
                for Entry {
                    tab,
                    index,
                    location,
                    label,
                } in entries
                {
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(label).icon(location.icon()).on_click(
                        move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                let valid = this.current_tab().is_some_and(|slot| {
                                    slot.id == tab
                                        && slot
                                            .navigation
                                            .history()
                                            .get(index)
                                            .is_some_and(|visit| visit.location == location)
                                });
                                this.focus.focus(window, cx);
                                if valid {
                                    this.navigate_history_entry(tab, index, cx);
                                }
                                cx.notify();
                            });
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }

    fn prepare_history_menu(
        &mut self,
        direction: NavigationDirection,
        cx: &mut Context<Self>,
    ) -> Vec<Entry> {
        let Some(slot) = self.current_tab() else {
            return Vec::new();
        };
        let navigation = &slot.navigation;
        let current = navigation.history_index();
        let indices: Vec<_> = match direction {
            NavigationDirection::Back => (0..current).rev().collect(),
            NavigationDirection::Forward => (current + 1..navigation.history().len()).collect(),
        };
        let entries = indices
            .into_iter()
            .map(|index| {
                let location = navigation.history()[index].location.clone();
                let label = self
                    .resolve_location(&location, cx)
                    .path
                    .map_or_else(|| location.label(cx), |path| path.display().to_string());
                Entry {
                    tab: slot.id,
                    index,
                    location,
                    label,
                }
            })
            .collect();
        self.discard_marquee();
        self.menu_open = false;
        self.keyboard_menu = None;
        self.sort_menu = None;
        cx.notify();
        entries
    }
}
