//! The application menu: the toolbar control and the popup it opens.
//!
//! The button and the keyboard shortcut drive one open flag on the workspace,
//! so `Alt+M` (with `F10` as its compatibility stroke) opens exactly the menu
//! the pointer opens, and can never leave a second one behind.

use super::*;
use gds::ui::{self, Shortcut, button::Button};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    WindowExt,
    dialog::AlertDialog,
    menu::{PopupMenu, PopupMenuItem},
    popover::{Popover, PopoverState},
};
use gpui_kit::prelude::*;

/// One row of the application menu.
///
/// Settings is a workspace command, so its row runs the same dispatch path as
/// every other control for it and shows the same live binding. About belongs to
/// the menu alone: it has no command of its own, which keeps the navigation
/// actions the single owner of that concern.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuEntry {
    Settings,
    Logs,
    About,
}

impl MenuEntry {
    /// The rows of the application menu, in order.
    const ROWS: [Self; 3] = [Self::Settings, Self::Logs, Self::About];

    fn label_id(self) -> &'static str {
        match self {
            Self::Settings => Command::Settings.label_id(),
            Self::Logs => "logs-title",
            Self::About => "about",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Settings => IconName::Settings,
            Self::Logs => IconName::List,
            Self::About => IconName::Info,
        }
    }

    /// The command whose binding the row shows, if the row has one.
    fn command(self) -> Option<Command> {
        match self {
            Self::Settings => Some(Command::Settings),
            Self::About | Self::Logs => None,
        }
    }

    /// The row's label and, for a command row, its live key badge.
    fn render(self) -> impl Fn(&mut Window, &mut App) -> AnyElement {
        move |window, cx| {
            let shortcut = if self == Self::Logs {
                Some(Shortcut::new(Box::new(gds::ui::logs::OpenLogs), None))
            } else {
                self.command().map(Into::into)
            };
            ui::menu::row(i18n::text(self.label_id(), cx), shortcut)(window, cx)
        }
    }

    /// The press that runs the row; the popup closes itself afterwards.
    fn on_click(self, view: WeakEntity<Browser>) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |event, window, cx| match self {
            Self::Settings => {
                Browser::menu_command(view.clone(), Command::Settings)(event, window, cx);
            }
            Self::About => Browser::show_about(view.clone(), window, cx),
            Self::Logs => {
                let _ = view.update(cx, |browser, cx| browser.set_menu_open(false, cx));
                window.dispatch_action(Box::new(gds::ui::logs::OpenLogs), cx);
            }
        }
    }
}

/// The menu entity kept for one open session.
///
/// The popover's content builder runs on every frame the menu is open, so the
/// menu is built once and reused: rebuilding it per frame would replace its
/// focus handle and its selection.
#[derive(Default)]
struct MenuState {
    menu: Option<Entity<PopupMenu>>,
}

/// The toolbar menu control together with the popup it opens.
///
/// `open` is the workspace's flag, so the pointer and the shortcut go through
/// the same state: a press can only open this popup, and a press while it is up
/// closes it instead of stacking another menu.
pub(super) fn application_menu(
    open: bool,
    view: WeakEntity<Browser>,
    cx: &mut Context<Browser>,
) -> AnyElement {
    let application = gds::ui::about::Application::cargo(env!("CARGO_BIN_NAME"));
    let label = i18n::text("app-menu", cx);
    let trigger = Button::application_menu("application-menu", application.icon)
        .accessibility_label(label.clone())
        .tooltip_with_shortcut(label, Shortcut::from(Command::Menu));
    let opener = view.clone();
    Popover::new("application-menu-popover")
        .appearance(false)
        .overlay_closable(false)
        .open(open)
        .trigger(trigger)
        .on_open_change(move |open, _, cx| {
            if let Some(view) = opener.upgrade() {
                view.update(cx, |browser, cx| browser.set_menu_open(*open, cx));
            }
        })
        .content(move |_, window, cx| popup(view.clone(), window, cx))
        .into_any_element()
}

impl Browser {
    /// Record whether the application menu is showing.
    pub(super) fn set_menu_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.menu_open == open {
            return;
        }
        self.menu_open = open;
        cx.notify();
    }

    /// Show the about dialog over the file workspace.
    ///
    /// It opens on the next frame, after the menu has left the focus tree, so
    /// the dialog records the browser as its return target and gives focus back
    /// there when it closes.
    pub(super) fn show_about(view: WeakEntity<Self>, window: &mut Window, _: &mut App) {
        window.on_next_frame(move |window, cx| {
            let Some(view) = view.upgrade() else {
                return;
            };
            let focus = view.read(cx).focus.clone();
            focus.focus(window, cx);
            let about = cx.new(|cx| {
                gds::ui::about::About::new(
                    gds::ui::about::Application::cargo(env!("CARGO_BIN_NAME")),
                    cx,
                )
            });
            window.open_alert_dialog(cx, move |dialog: AlertDialog, _, cx| {
                let footer = ui::dialog::DialogFooter::default().px_4().pb_4().child(
                    ui::dialog::DialogDecision::new(ui::dialog::DialogCommand::Cancel)
                        .label(i18n::text("close", cx)),
                );
                el! {
                    dialog {
                        p_0; footer: footer;
                    } [
                        .div { h: gds::ui::about::HEIGHT.px; min_h_0; } [$about.clone()],
                    ]
                }
            });
        });
    }
}

/// The open menu, built once per open session.
fn popup(
    view: WeakEntity<Browser>,
    window: &mut Window,
    cx: &mut Context<PopoverState>,
) -> AnyElement {
    let state = window.use_keyed_state(SharedString::from("application-menu:menu"), cx, |_, _| {
        MenuState::default()
    });
    if let Some(menu) = state.read(cx).menu.clone() {
        return menu.into_any_element();
    }
    let build_view = view.clone();
    let menu = PopupMenu::build(window, cx, move |menu, _, cx| rows(menu, build_view, cx));
    state.update(cx, |state, _| state.menu = Some(menu.clone()));
    // A controlled popover applies its own focus while mounting. Give the
    // menu focus afterwards so its Escape and arrow handlers own the keys.
    let opening_menu = menu.downgrade();
    let opening_view = view.clone();
    window.on_next_frame(move |window, cx| {
        if opening_view
            .upgrade()
            .is_some_and(|view| view.read(cx).menu_open)
            && let Some(menu) = opening_menu.upgrade()
        {
            menu.focus_handle(cx).focus(window, cx);
        }
    });
    let popover = cx.entity().downgrade();
    window
        .subscribe(&menu, cx, {
            // The state owns the menu, so a strong handle here would close the
            // cycle `state -> menu -> listener -> state`.
            let state = state.downgrade();
            move |_, _: &DismissEvent, window, cx| {
                _ = popover.update(cx, |popover, cx| popover.dismiss(window, cx));
                _ = state.update(cx, |state, _| state.menu = None);
                if let Some(view) = view.upgrade() {
                    view.update(cx, |browser, cx| browser.set_menu_open(false, cx));
                }
            }
        })
        .detach();
    menu.into_any_element()
}

/// Application pages and the about entry.
fn rows(mut menu: PopupMenu, view: WeakEntity<Browser>, cx: &App) -> PopupMenu {
    menu = ui::menu::popup(menu);
    for entry in MenuEntry::ROWS
        .into_iter()
        .filter(|entry| *entry != MenuEntry::Logs || gds::ui::logs::is_enabled(cx))
    {
        let render = entry.render();
        let on_click = entry.on_click(view.clone());
        menu = menu.item(
            PopupMenuItem::element(render)
                .icon(entry.icon())
                .on_click(on_click),
        );
    }
    menu
}

#[cfg(test)]
mod tests;
