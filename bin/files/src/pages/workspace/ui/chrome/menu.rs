//! The application menu: the toolbar control and the popup it opens.
//!
//! The button and the keyboard shortcut drive the same menu on the workspace,
//! so `Alt+M` (with `F10` as its compatibility stroke) opens exactly the menu
//! the pointer opens, and can never leave a second one behind.

mod overflow;

use super::*;
use gds::ui::about::About;
use gds::ui::about::Application;
use gds::ui::dialog::DialogCommand;
use gds::ui::dialog::DialogDecision;
use gds::ui::dialog::DialogFooter;
use gds::ui::logs::OpenLogs;
use gds::ui::{self, Shortcut, button::Button};
use gds_uim::{component, el};
use gpui_kit::IntoElement;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    WindowExt,
    dialog::AlertDialog,
    menu::{PopupMenu, PopupMenuItem},
    popover::{Popover, PopoverState},
};
use gpui_kit::prelude::*;

#[derive(Default)]
pub(in crate::pages::workspace) struct State {
    /// Only one of the application menu and page tools can be open.
    pub open: Option<Kind>,
    pub context: Option<context_menu::OpenMenu>,
    pub sort: Option<sort::OpenMenu>,
    pub volume_focus: Option<FocusHandle>,
}

impl State {
    pub(in crate::pages::workspace) fn is_any_open(&self) -> bool {
        self.open.is_some() || self.context.is_some() || self.sort.is_some()
    }

    pub(in crate::pages::workspace) fn close_all(&mut self) {
        self.open = None;
        self.context = None;
        self.sort = None;
    }

    pub(in crate::pages::workspace) fn close_for_resize(&mut self) -> bool {
        let page_tools_open = self.open == Some(Kind::PageTools);
        let menu_open = page_tools_open || self.sort.is_some();
        if page_tools_open {
            self.open = None;
        }
        self.sort = None;
        menu_open
    }

    pub(in crate::pages::workspace) fn dismiss_for_command(&mut self, command: Command) -> bool {
        let mut dismissed = false;
        if self.open.is_some() && command != Command::Menu {
            self.open = None;
            dismissed = true;
        }
        if self.context.is_some() && command != Command::ContextMenu {
            self.context = None;
            dismissed = true;
        }
        if self.sort.is_some() && command != Command::Sort {
            self.sort = None;
            dismissed = true;
        }
        dismissed
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::pages::workspace) enum Kind {
    #[default]
    Application,
    PageTools,
}

/// One row of the application menu.
///
/// Settings is a workspace command, so its row runs the same dispatch path as
/// every other control for it and shows the same live binding. About belongs to
/// the menu alone: it has no command of its own, which keeps the navigation
/// actions the single owner of that concern.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuEntry {
    NewWindow,
    Settings,
    Help,
    Logs,
    About,
}

impl MenuEntry {
    /// The rows of the application menu, in order.
    const ROWS: [Self; 5] = [
        Self::NewWindow,
        Self::Settings,
        Self::Help,
        Self::Logs,
        Self::About,
    ];

    fn label_id(self) -> &'static str {
        match self {
            Self::NewWindow => Command::NewWindow.label_id(),
            Self::Settings => Command::Settings.label_id(),
            Self::Help => Command::Help.label_id(),
            Self::Logs => "logs-title",
            Self::About => "about",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::NewWindow => IconName::Plus,
            Self::Settings => IconName::Settings,
            Self::Help => IconName::BookOpen,
            Self::Logs => IconName::List,
            Self::About => IconName::Info,
        }
    }

    /// The command whose binding the row shows, if the row has one.
    fn command(self) -> Option<Command> {
        match self {
            Self::NewWindow => Some(Command::NewWindow),
            Self::Settings => Some(Command::Settings),
            Self::Help => Some(Command::Help),
            Self::About | Self::Logs => None,
        }
    }

    /// The row's label and, for a command row, its live key badge.
    fn render(self) -> impl Fn(&mut Window, &mut App) -> AnyElement {
        move |window, cx| {
            let shortcut = if self == Self::Logs {
                Some(Shortcut::new(Box::new(OpenLogs), None))
            } else {
                self.command().map(Into::into)
            };
            ui::menu::row(i18n::text(self.label_id(), cx), shortcut)(window, cx)
        }
    }

    /// The press that runs the row; the popup closes itself afterwards.
    fn on_click(self, view: WeakEntity<Browser>) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |event, window, cx| match self {
            Self::NewWindow => {
                Browser::menu_command(view.clone(), Command::NewWindow)(event, window, cx)
            }
            Self::Settings => {
                Browser::menu_command(view.clone(), Command::Settings)(event, window, cx);
            }
            Self::Help => Browser::menu_command(view.clone(), Command::Help)(event, window, cx),
            Self::About => Browser::show_about(view.clone(), window, cx),
            Self::Logs => {
                let _ = view.update(cx, |browser, cx| browser.set_menu_open(None, cx));
                window.dispatch_action(Box::new(OpenLogs), cx);
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
/// `open` identifies the workspace popup, so the pointer and the shortcut go through
/// the same state: a press can only open this popup, and a press while it is up
/// closes it instead of stacking another menu.
#[component]
#[derive(Default, IntoElement)]
pub(in crate::pages::workspace) struct MenuControl {
    #[attr]
    kind: Kind,
    #[attr]
    open: Option<Kind>,
    #[attr(option)]
    view: Option<WeakEntity<Browser>>,
}

impl RenderOnce for MenuControl {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(view) = self.view else {
            return el! { .div }.into_any_element();
        };
        let kind = self.kind;
        let application = Application::cargo(env!("CARGO_BIN_NAME"));
        let compact = kind == Kind::PageTools;
        let label = i18n::text(if compact { "page-tools" } else { "app-menu" }, cx);
        let trigger = if compact {
            el! { "page-tools".Button::icon_button {
                control_icon: IconName::Ellipsis; tooltip: ~label;
            } }
        } else {
            el! { ("application-menu", $(application.icon)).Button::application_menu {
                tooltip_with_shortcut: ~label, $(Shortcut::from(Command::Menu));
            } }
        };
        let trigger = el! { trigger { accessibility_label: label; } };
        let opener = view.clone();
        let on_open_change = move |open: &bool, _: &mut Window, cx: &mut App| {
            if let Some(view) = opener.upgrade() {
                view.update(cx, |browser, cx| {
                    if *open {
                        browser.set_menu_open(Some(kind), cx);
                    } else if browser.menus.open == Some(kind) {
                        browser.set_menu_open(None, cx);
                    }
                });
            }
        };
        let content =
            move |_: &mut PopoverState, window: &mut Window, cx: &mut Context<PopoverState>| {
                popup(kind, view.clone(), window, cx)
            };
        let id = if compact {
            "compact-menu-popover"
        } else {
            "application-menu-popover"
        };
        el! { id.Popover::new {
            appearance: false; overlay_closable: false; open: $(self.open == Some(kind));
            trigger: trigger; on_open_change: on_open_change; content: content;
        } }
        .into_any_element()
    }
}

impl Browser {
    /// Select one popup, or close the current popup.
    pub(in crate::pages::workspace) fn set_menu_open(
        &mut self,
        open: Option<Kind>,
        cx: &mut Context<Self>,
    ) {
        if self.menus.open == open {
            return;
        }
        if open.is_some() {
            self.discard_marquee();
        }
        self.menus.open = open;
        cx.notify();
    }

    /// Show the about dialog over the file workspace.
    ///
    /// It opens on the next frame, after the menu has left the focus tree, so
    /// the dialog records the browser as its return target and gives focus back
    /// there when it closes.
    pub(in crate::pages::workspace) fn show_about(
        view: WeakEntity<Self>,
        window: &mut Window,
        _: &mut App,
    ) {
        window.on_next_frame(move |window, cx| {
            let Some(view) = view.upgrade() else {
                return;
            };
            let focus = view.read(cx).window.focus.clone();
            focus.focus(window, cx);
            let about = cx.new(|cx| {
                About::new(
                    Application::cargo(env!("CARGO_BIN_NAME")),
                    cx,
                )
            });
            window.open_alert_dialog(cx, move |dialog: AlertDialog, _, cx| {
                let footer = el! {
                    .DialogFooter::default { px_4; pb_4; } [
                        DialogCommand::Cancel.DialogDecision::new { label: $(i18n::text("close", cx)); },
                    ]
                };
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
    kind: Kind,
    view: WeakEntity<Browser>,
    window: &mut Window,
    cx: &mut Context<PopoverState>,
) -> AnyElement {
    let compact = kind == Kind::PageTools;
    let key = if compact {
        "compact-menu:menu"
    } else {
        "application-menu:menu"
    };
    let state = window.use_keyed_state(SharedString::from(key), cx, |_, _| MenuState::default());
    if let Some(menu) = state.read(cx).menu.clone() {
        return menu.into_any_element();
    }
    let build_view = view.clone();
    let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
        if kind == Kind::PageTools {
            overflow::rows(menu, build_view, window, cx)
        } else {
            rows(menu, build_view, cx)
        }
    });
    state.update(cx, |state, _| state.menu = Some(menu.clone()));
    // A controlled popover applies its own focus while mounting. Give the
    // menu focus afterwards so its Escape and arrow handlers own the keys.
    let opening_menu = menu.downgrade();
    let opening_view = view.clone();
    window.on_next_frame(move |window, cx| {
        if opening_view
            .upgrade()
            .is_some_and(|view| view.read(cx).menus.open == Some(kind))
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
                    view.update(cx, |browser, cx| {
                        if browser.menus.open == Some(kind) {
                            browser.set_menu_open(None, cx);
                        }
                    });
                }
            }
        })
        .detach();
    menu.into_any_element()
}

/// Application pages and the about entry.
fn rows(mut menu: PopupMenu, view: WeakEntity<Browser>, cx: &App) -> PopupMenu {
    menu = ui::menu::popup(menu);
    for command in [Command::Undo, Command::Redo] {
        let browser = view.upgrade();
        let enabled = browser
            .as_ref()
            .is_some_and(|view| view.read(cx).history_available(command, cx));
        let label = browser
            .as_ref()
            .map(|view| view.read(cx).history_label(command, cx))
            .unwrap_or_else(|| i18n::text(command.label_id(), cx));
        menu = menu.item(
            ui::menu::item(label, Some(command.into()), enabled)
                .icon(if command == Command::Undo {
                    IconName::Undo2
                } else {
                    IconName::Redo2
                })
                .on_click(Browser::menu_command(view.clone(), command)),
        );
    }
    menu = menu.separator();
    for entry in MenuEntry::ROWS
        .into_iter()
        .filter(|entry| *entry != MenuEntry::Logs || cx.gds().ui().logs().is_enabled())
    {
        let render = entry.render();
        let on_click = entry.on_click(view.clone());
        menu = menu.item(el! { render.PopupMenuItem::element {
            icon: $(entry.icon());
            on_click: on_click;
        } });
    }
    menu
}
