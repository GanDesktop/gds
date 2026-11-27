//! Window toolbar controls and back/forward/parent navigation buttons.

use std::path::Path;

use gds::i18n;
use gds::ui::IconName;
use gds::ui::button::IconButtonProps;
use gds::ui::frame::WindowDragAreaProps;
use gds::ui::settings::SubpageToolbarTitleProps;
use gds::ui::window::{HeightTier, WidthTier};
use zgui::prelude::*;
use zgui::view::{events, install_stylesheet as install_sheet};
use zgui::{css, view};

use super::{Chrome, install_chrome_stylesheets};
use crate::entities::location::{Location, scheme};
use crate::pages::workspace::{
    navigation::Navigator,
    panes::MenuHost,
    projection::{Geometry, UiState},
    state::{context::local_memo, model::Pane},
};
use crate::shared::ui::actions::Command;

const TOOLBAR_SHEET: &str = "gds-files-chrome-toolbar";

const TOOLBAR_CSS: &str = css!(
    r#"
    .gds-files-toolbar-leading.gds-files-toolbar-leading {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
    }

    .gds-files-menu-controls.gds-files-menu-controls {
        display: flex;
        align-items: center;
        justify-content: center;
        flex: none;
    }

    .gds-files-toolbar.gds-files-toolbar {
        display: flex;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        align-items: center;
        gap: 4px;
        min-width: 0px;
    }

    .gds-files-toolbar-files.gds-files-toolbar-files {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        gap: 4px;
        min-width: 0px;
    }

    .gds-files-nav-group.gds-files-nav-group {
        display: flex;
        flex: none;
        align-items: center;
        gap: 4px;
    }
    "#
);

pub(super) fn install_stylesheet() {
    install_sheet(TOOLBAR_SHEET, TOOLBAR_CSS);
}

pub(in crate::pages::workspace) fn subpage_toolbar_title(label_key: &'static str) -> AnyView {
    AnyView::new(view! {
        SubpageToolbarTitle(label = Signal::derive_local(move || i18n::text(label_key)))
    })
}

impl Chrome<'_> {
    pub(in crate::pages::workspace) fn menu_controls(&self) -> AnyView {
        install_chrome_stylesheets();
        let ctx = self.0.clone();
        let no_drag = self.0.0.window.as_ref().map(WindowHandle::no_drag_handler);
        AnyView::new(view! {
            row(
                class = "gds-files-menu-controls",
                on:pointer_down = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerDown>| {
                    if let Some(ref handler) = no_drag {
                        handler(ev);
                    } else {
                        ev.stop_propagation();
                    }
                }
            ) {
                {{move || {
                    if ctx.pane() == Pane::Files {
                        MenuHost(&ctx).app_menu()
                    } else {
                        let back = ctx.clone();
                        AnyView::new(view! {
                            IconButton(
                                icon = IconName::ArrowLeft,
                                label = Signal::derive_local(|| i18n::text("page-return")),
                                shortcut = Some(Command::Cancel.shortcut()),
                                on:click = move |_| back.back_page()
                            )
                        })
                    }
                }}}
            }
        })
    }

    /// Leading `sidebar_width` toolbar cell (`[ApplicationMenuButton / ArrowLeft] + [WindowDragArea]`).
    pub(in crate::pages::workspace) fn toolbar_leading(&self) -> AnyView {
        install_chrome_stylesheets();
        let menu = self.menu_controls();
        AnyView::new(view! {
            row(class = "gds-files-toolbar-leading") {
                {menu}
                WindowDragArea() { box() }
            }
        })
    }

    /// Right-column toolbar controls (`[PanelLeft, Back, Forward, Parent, flex:1 AddressBar, optional Micro ... menu]`).
    pub(in crate::pages::workspace) fn toolbar_controls(&self) -> AnyView {
        install_chrome_stylesheets();
        let ctx = self.0.clone();
        let toggle_c = self.0.clone();
        let width_c = self.0.clone();
        let height_c = self.0.clone();
        let pane_c = self.0.clone();
        let micro_width =
            Signal::derive_local(move || Geometry(&width_c).width_tier() == WidthTier::Micro);
        let micro_height =
            Signal::derive_local(move || Geometry(&height_c).height_tier() == HeightTier::Micro);
        let pane_sig = local_memo(move || pane_c.pane());
        let show_sidebar_toggle =
            Signal::derive_local(move || pane_sig.get() != Pane::Help && !micro_width.get());
        let no_drag = self.0.0.window.as_ref().map(WindowHandle::no_drag_handler);

        AnyView::new(view! {
            row(class = "gds-files-toolbar") {
                Show(when = move || show_sidebar_toggle.get()) {
                    {{
                        let click_c = toggle_c.clone();
                        let no_drag_btn = no_drag;
                        view! {
                            IconButton(
                                icon = IconName::PanelLeft,
                                label = Signal::derive_local(|| i18n::text("sidebar-toggle")),
                                shortcut = Some(Command::Sidebar.shortcut()),
                                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                                    if let Some(ref handler) = no_drag_btn {
                                        handler(ev);
                                    } else {
                                        ev.stop_propagation();
                                    }
                                },
                                on:click = move |_| {
                                    UiState(&click_c).toggle_sidebar();
                                }
                            )
                        }
                    }}
                }
                {{move || match pane_sig.get() {
                    Pane::Files => {
                        let nav_c = ctx.clone();
                        let addr_c = ctx.clone();
                        let menu_c = ctx.clone();
                        let no_drag_menu = no_drag;
                        AnyView::new(view! {
                            row(class = "gds-files-toolbar-files") {
                                Show(when = move || !micro_width.get()) {
                                    {{Chrome(&nav_c).navigation_buttons()}}
                                }
                                {{Chrome(&addr_c).address_bar()}}
                                Show(when = move || micro_height.get()) {
                                    {{
                                        let mc = menu_c.clone();
                                        view! {
                                            row(
                                                class = "gds-files-menu-controls",
                                                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                                                    if let Some(ref handler) = no_drag_menu {
                                                        handler(ev);
                                                    } else {
                                                        ev.stop_propagation();
                                                    }
                                                }
                                            ) {
                                                {MenuHost(&mc).page_menu()}
                                            }
                                        }
                                    }}
                                }
                            }
                        })
                    }
                    Pane::Settings => subpage_toolbar_title("settings"),
                    Pane::Help => AnyView::new(view! { box() }),
                }}}
            }
        })
    }

    fn navigation_buttons(&self) -> AnyView {
        let ctx = self.0.clone();
        let state_sig = Signal::derive_local(move || {
            ctx.read(|state| {
                let tab = state.tab();
                let backward = tab.navigation.back_entries();
                let forward = tab.navigation.forward_entries();
                let can_up = scheme::parent(tab.navigation.location()).is_some()
                    || tab.navigation.path().and_then(Path::parent).is_some();
                (tab.id, backward, forward, can_up)
            })
        });
        let parent_ctx = self.0.clone();

        AnyView::new(view! {
            row(class = "gds-files-nav-group") {
                {{move || {
                    let (tab, backward, forward, can_up) = state_sig.get();
                    let c_click = parent_ctx.clone();
                    view! {
                        row(class = "gds-files-nav-group") {
                            {{Chrome(&parent_ctx).history_button(tab, backward, false)}}
                            {{Chrome(&parent_ctx).history_button(tab, forward, true)}}
                            IconButton(
                                icon = IconName::ArrowUp,
                                label = Signal::derive_local(|| i18n::text("parent")),
                                disabled = Signal::stored_local(!can_up),
                                shortcut = Some(Command::Parent.shortcut()),
                                on:click = move |_| Navigator(&c_click).parent()
                            )
                        }
                    }
                }}}
            }
        })
    }

    fn history_button(&self, tab: u64, visits: Vec<(usize, Location)>, forward: bool) -> AnyView {
        let disabled = visits.is_empty();
        let icon = if forward {
            IconName::ArrowRight
        } else {
            IconName::ArrowLeft
        };
        let label_key = if forward { "forward" } else { "back" };
        let shortcut = if forward {
            Command::Forward.shortcut()
        } else {
            Command::Back.shortcut()
        };
        let click_ctx = self.0.clone();

        let button = AnyView::new(view! {
            IconButton(
                icon = icon,
                label = Signal::derive_local(move || i18n::text(label_key)),
                disabled = Signal::stored_local(disabled),
                shortcut = Some(shortcut),
                on:click = move |_| Navigator(&click_ctx).history(forward)
            )
        });
        if disabled {
            return button;
        }

        MenuHost(self.0).history_menu(button, tab, visits)
    }
}
