//! One sort menu shared by the toolbar and its keyboard shortcut.
use super::*;
use gds::ui::{Shortcut, button::Button};
use gpui_kit::{
    assets::IconName,
    component::{
        menu::{PopupMenu, PopupMenuItem},
        popover::Popover,
    },
    prelude::*,
};

pub(super) struct OpenMenu {
    menu: Entity<PopupMenu>,
    _dismiss: Subscription,
}

impl Browser {
    pub(super) fn set_sort_menu_open(
        &mut self,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if open == self.menus.sort.is_some() {
            return;
        }
        if !open {
            self.menus.sort = None;
            self.window.focus.focus(window, cx);
            cx.notify();
            return;
        }
        if !self.pages.is_empty()
            || self.collection().is_some()
            || self.editing_path(window, cx)
            || window.has_active_dialog(cx)
        {
            return;
        }
        let Some(current) = self.active().map(|slot| slot.sort) else {
            return;
        };
        self.discard_marquee();
        self.menus.context = None;
        self.menus.application_open = false;
        let browser = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, move |menu, _, cx| {
            let mut menu = ui::menu::popup(menu);
            for (key, label) in [
                (sorting::Key::Name, "name"),
                (sorting::Key::Type, "type"),
                (sorting::Key::Size, "size"),
                (sorting::Key::Modified, "modified"),
            ] {
                let browser = browser.clone();
                menu = menu.item(
                    PopupMenuItem::new(i18n::text(label, cx))
                        .checked(current.key == key)
                        .on_click(move |_, _, cx| {
                            let _ = browser.update(cx, |this, cx| {
                                this.set_sort(
                                    sorting::Sort {
                                        key,
                                        direction: current.direction,
                                    },
                                    cx,
                                );
                            });
                        }),
                );
            }
            menu.separator().item(
                PopupMenuItem::new(i18n::text("sort-descending", cx))
                    .checked(current.direction == sorting::Direction::Descending)
                    .on_click(move |_, _, cx| {
                        let _ = browser
                            .update(cx, |this, cx| this.set_sort(current.next(current.key), cx));
                    }),
            )
        });
        let menu_id = menu.entity_id();
        let dismiss = cx.subscribe_in(&menu, window, move |this, _, _: &DismissEvent, _, cx| {
            if this
                .menus
                .sort
                .as_ref()
                .is_some_and(|open| open.menu.entity_id() == menu_id)
            {
                this.menus.sort = None;
                cx.notify();
            }
        });
        self.menus.sort = Some(OpenMenu {
            menu: menu.clone(),
            _dismiss: dismiss,
        });
        let browser = cx.entity().downgrade();
        let menu = menu.downgrade();
        window.on_next_frame(move |window, cx| {
            if browser.upgrade().is_some_and(|browser| {
                browser
                    .read(cx)
                    .menus
                    .sort
                    .as_ref()
                    .is_some_and(|open| open.menu.entity_id() == menu_id)
            }) && let Some(menu) = menu.upgrade()
            {
                menu.focus_handle(cx).focus(window, cx);
            }
        });
        cx.notify();
    }

    pub(super) fn sort_button(&self, cx: &mut Context<Self>) -> Popover {
        let label = i18n::text("sort", cx);
        let trigger = Button::icon_button("sort")
            .control_icon(IconName::ArrowDownWideNarrow)
            .accessibility_label(label.clone())
            .tooltip_with_shortcut(label, Shortcut::from(Command::Sort));
        let menu = self.menus.sort.as_ref().map(|open| open.menu.clone());
        let on_open =
            cx.listener(|this, open: &bool, window, cx| this.set_sort_menu_open(*open, window, cx));
        Popover::new("file-sort-popover")
            .appearance(false)
            .overlay_closable(false)
            .open(menu.is_some())
            .trigger(trigger)
            .on_open_change(on_open)
            .content(move |_, _, _| {
                menu.clone()
                    .map(IntoElement::into_any_element)
                    .unwrap_or_else(|| div().into_any_element())
            })
    }
}
