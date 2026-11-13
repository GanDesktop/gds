//! The Files window owns terminal sessions independently of file locations.

use gds::appearance::tokens;
use gds::ui::IconButton;
use gds::ui::sidebar::{SideBarSectionTitle, SidebarPresentation};
use gds::ui::split::VerticalSplit;
mod panel;

use super::*;
use gds::i18n::t;
use gds::ui::Frame;
use gds_uim::el;
use gpui_kit::{assets::IconName, component::ActiveTheme, prelude::*};
use panel::Panel;

// Keep a tab, the optional volume group, and at least one location reachable.
pub(in crate::pages::workspace) const MIN_CONTENT_HEIGHT: f32 = sidebar::MIN_TABS_HEIGHT
    + sidebar::MIN_DEVICES_HEIGHT
    + ui::split::DIVIDER_HEIGHT
    + tokens::SIDEBAR_ITEM_HEIGHT * 2.;
pub(in crate::pages::workspace) const MIN_DOCK_HEIGHT: f32 = 160.;
pub(in crate::pages::workspace) const DEFAULT_DOCK_HEIGHT: f32 = 280.;
const COLLAPSED_ACTION_INSET: f32 =
    (tokens::COLLAPSED_SIDEBAR_WIDTH - 2. * tokens::SIDEBAR_PADDING - tokens::ICON_BUTTON_SIZE)
        / 2.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::pages::workspace) enum Lifecycle {
    #[default]
    Active,
    Closing,
}

/// Sessions and selection belong to this window, never a file location.
pub(in crate::pages::workspace) struct State {
    pub dock: Option<Dock>,
    focus: FocusHandle,
    pub visible: bool,
    pub maximized: bool,
    pub split: Entity<ui::split::State>,
    pub lifecycle: Lifecycle,
}

impl State {
    pub(in crate::pages::workspace) fn new(
        preferences: &configuration::Preferences,
        cx: &mut App,
    ) -> Self {
        Self {
            dock: None,
            focus: cx.focus_handle(),
            visible: false,
            maximized: false,
            split: cx.new(|cx| {
                let mut split = ui::split::State::new(
                    preferences.files.terminal_content_height,
                    MIN_CONTENT_HEIGHT,
                    MIN_DOCK_HEIGHT,
                    cx,
                );
                split.default_bottom(DEFAULT_DOCK_HEIGHT);
                split
            }),
            lifecycle: Lifecycle::Active,
        }
    }
}

pub(in crate::pages::workspace) struct Dock {
    pub panel: Entity<Panel>,
    cwd: Option<PathBuf>,
    sidebar: SidebarPresentation,
    _changes: Subscription,
}

/// Only a verified available directory in the loaded workspace state is a local launch target.
fn local_cwd(path: &Path, available: bool) -> Option<PathBuf> {
    (available && path.is_absolute()).then(|| path.to_path_buf())
}

fn launch_cwd(path: Option<&Path>, available: bool, home: &Path) -> PathBuf {
    path.and_then(|path| local_cwd(path, available))
        .unwrap_or_else(|| home.to_path_buf())
}

impl Browser {
    pub(in crate::pages::workspace) fn terminal_visible(&self) -> bool {
        self.settings.preferences.terminal_enabled()
            && self.terminal.visible
            && self.picker.is_none()
            && self.pages.is_empty()
    }

    pub(in crate::pages::workspace) fn terminal_focused(&self, window: &Window, cx: &App) -> bool {
        self.terminal.focus.contains_focused(window, cx)
            || self
                .terminal
                .dock
                .as_ref()
                .is_some_and(|dock| dock.panel.read(cx).contains_focus(window, cx))
    }

    fn terminal_cwd(&self, cx: &App) -> Option<PathBuf> {
        if self.picker.is_some() || !self.settings.preferences.terminal_enabled() {
            return None;
        }
        let slot = self.active();
        Some(launch_cwd(
            slot.and_then(|slot| slot.path()),
            slot.is_some_and(|slot| {
                slot.navigation.directory().is_some()
                    && slot.files_available()
                    && slot
                        .path()
                        .is_some_and(|path| self.device_path_available(path, cx))
            }),
            &self.locations.home,
        ))
    }

    /// Creating the window's empty panel does not start a shell.
    fn ensure_terminal(&mut self, cx: &mut Context<Self>) -> Option<Entity<Panel>> {
        if self.picker.is_some() || !self.settings.preferences.terminal_enabled() {
            return None;
        }
        if self.terminal.lifecycle == Lifecycle::Closing {
            return None;
        }
        if let Some(dock) = &self.terminal.dock {
            return Some(dock.panel.clone());
        }
        let cwd = self.terminal_cwd(cx);
        let sidebar = self.sidebar.presentation;
        let preferences = self.settings.preferences.terminal.clone();
        let panel = cx.new(|cx| {
            let mut panel = Panel::new(cx);
            panel.set_preferences(preferences, cx);
            panel.set_cwd(cwd.clone(), cx);
            panel.set_sidebar_presentation(sidebar, cx);
            panel
        });
        let changes = cx.observe(&panel, |_, _, cx| cx.notify());
        self.terminal.dock = Some(Dock {
            panel: panel.clone(),
            cwd,
            sidebar,
            _changes: changes,
        });
        Some(panel)
    }

    pub(in crate::pages::workspace) fn toggle_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.ensure_terminal(cx) else {
            return;
        };
        self.pages.clear();

        self.toolbar.finish_path_edit();
        self.terminal.visible = !self.terminal.visible;
        if self.terminal.visible {
            panel.update(cx, |panel, cx| panel.reveal(window, cx));
        } else {
            self.window.focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(in crate::pages::workspace) fn open_internal_terminal(
        &mut self,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.picker.is_some() || !self.pages.is_empty() {
            return;
        }
        let known_directory = self.active().is_some_and(|slot| {
            slot.files_available()
                && (slot.path() == Some(cwd.as_path())
                    || slot.table_rows.iter().any(|entry| {
                        entry.path == cwd && super::file_actions::is_directory_entry(entry)
                    }))
        });
        let available = known_directory && self.device_path_available(&cwd, cx);
        let cwd = launch_cwd(Some(&cwd), available, &self.locations.home);
        let Some(panel) = self.ensure_terminal(cx) else {
            return;
        };

        self.toolbar.finish_path_edit();
        self.terminal.visible = true;
        // Each request starts a fresh session; existing PTYs are never redirected.
        panel.update(cx, |panel, cx| panel.open(cwd, window, cx));
        cx.notify();
    }

    /// File navigation only changes the *next* new-session target. It never
    /// writes a command into the active PTY, even when the shell looks idle.
    pub(in crate::pages::workspace) fn sync_terminal_cwd(&mut self, cx: &mut Context<Self>) {
        if self.terminal_visible()
            && let Some(panel) = self.ensure_terminal(cx)
            && panel.read(cx).needs_initial_reveal()
        {
            // Defer PTY startup and recheck visibility so hiding the dock
            // cancels initial startup.
            let owner = cx.entity().downgrade();
            let window = self.window.handle;
            let _ = window.update(cx, |_, window, cx| {
                window.defer(cx, move |window, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        if this.terminal_visible()
                            && this
                                .terminal
                                .dock
                                .as_ref()
                                .is_some_and(|dock| dock.panel == panel)
                            && panel.read(cx).needs_initial_reveal()
                        {
                            panel.update(cx, |panel, cx| panel.reveal(window, cx));
                        }
                    });
                });
            });
        }
        let cwd = self.terminal_cwd(cx);
        let sidebar = self.sidebar.presentation;
        let Some(dock) = self.terminal.dock.as_mut() else {
            return;
        };
        if dock.cwd != cwd {
            dock.cwd = cwd.clone();
            dock.panel.update(cx, |panel, cx| panel.set_cwd(cwd, cx));
        }
        if dock.sidebar != sidebar {
            dock.sidebar = sidebar;
            dock.panel
                .update(cx, |panel, cx| panel.set_sidebar_presentation(sidebar, cx));
        }
    }

    pub(in crate::pages::workspace) fn on_return_from_terminal(
        &mut self,
        _: &ReturnFromTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) || self.operations.rename.read(cx).is_open() {
            return;
        }
        if !self.terminal_focused(window, cx) || !self.pages.is_empty() {
            cx.propagate();
            return;
        }
        self.restore_terminal_content(window, cx);
    }

    fn restore_terminal_content(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal.maximized = false;
        self.window.focus.focus(window, cx);
        Self::restore_file_focus(cx.entity().downgrade(), self.tabs.active_id, window);
        cx.notify();
    }

    fn switch_terminal_directory(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pages.is_empty() || self.collection().is_some() || window.has_active_dialog(cx) {
            return;
        }
        let Some(cwd) = self.terminal_cwd(cx) else {
            notifications::info(i18n::text("files-terminal-cwd-unavailable", cx), cx);
            return;
        };
        // Do not ensure/reveal here: this operation only targets an existing selection.
        let Some(dock) = self.terminal.dock.as_ref() else {
            return;
        };
        dock.panel
            .update(cx, |panel, cx| panel.change_directory(cwd, window, cx));
    }

    fn maximize_terminal(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dock) = self.terminal.dock.as_ref() else {
            return;
        };
        let panel = dock.panel.clone();
        self.terminal.maximized = !self.terminal.maximized;
        panel.update(cx, |panel, cx| panel.focus(window, cx));
        cx.notify();
    }

    pub(in crate::pages::workspace) fn terminal_workspace(
        &self,
        workspace: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.settings.preferences.terminal_enabled() {
            return workspace;
        }
        let progress =
            super::motion::state_progress("files-terminal", self.terminal_visible(), window, cx);
        let maximized_progress = super::motion::state_progress(
            "files-terminal-maximize",
            self.terminal.maximized,
            window,
            cx,
        );
        let Some(dock) = self.terminal.dock.as_ref() else {
            return el! { $(self.terminal.split.clone()).VerticalSplit::new {
                top: workspace;
                bottom_progress: 0.;
            } }
            .into_any_element();
        };
        let panel = dock.panel.clone();
        let cwd_available = dock.cwd.is_some();
        let maximized = self.terminal.maximized;
        let maximize = el! { "files-terminal-maximize".IconButton::new {
            icon: $(if maximized {
                IconName::Minimize2
            } else {
                IconName::Maximize2
            });
            label: $(i18n::text(
                if maximized && self.terminal_visible() {
                    "files-terminal-restore"
                } else {
                    "files-terminal-maximize"
                },
                cx,
            ));
            on_click: $(cx.listener(Self::maximize_terminal));
        } };
        let width = self.width();
        let frame = el! { .Frame::default { sidebar_width: width; } };
        let title = t!(@cx, "settings-terminal");
        let new_button = panel.read(cx).new_button(&panel, window, cx);
        let heading = el! {
            .div {
                # "files-terminal-title"; flex; items_center; h_full; min_w_0;
                px: tokens::SIDEBAR_PADDING.px; text_color: $(cx.theme().muted_foreground);
                pr: $(tokens::SIDEBAR_PADDING + self.sidebar.presentation.interpolate(0., COLLAPSED_ACTION_INSET)).px;
            } [
                .div { flex_1; min_w_0; overflow_hidden; opacity: $(self.sidebar.presentation.label_opacity()); } [
                    title.SideBarSectionTitle::new { px: tokens::SIDEBAR_SECTION_PADDING.px; },
                ],
                new_button,
            ]
        };
        let active_title = panel.read(cx).active_title(cx);
        let title_hint = panel
            .read(cx)
            .active_hint(cx)
            .unwrap_or_else(|| ui::tooltip::Details::new(active_title.clone()));
        let tooltip = move |window: &mut Window, cx: &mut App| title_hint.view(window, cx);
        let controls = el! {
            .div { flex; items_center; h_full; min_w_0; px: tokens::TERMINAL_CONTENT_INSET.px; gap_1; } [
                .div {
                    # "files-active-terminal-title"; flex_1; min_w_0; truncate;
                    text_size: tokens::SMALL; hoverable_tooltip: tooltip;
                } [active_title],
                "files-terminal-switch-directory".IconButton::new {
                    icon: IconName::FolderInput; label: $(i18n::text("files-terminal-switch-directory", cx));
                    disabled: $(self.collection().is_some() || !cwd_available
                        || !panel.read(cx).can_change_directory(cx));
                    on_click: $(cx.listener(Self::switch_terminal_directory));
                },
                maximize,
            ]
        };
        let header = el! {
            $(frame.workspace(heading.into_any_element(), controls.into_any_element(), cx)) {
                flex_none; h: tokens::TERMINAL_HEADER_HEIGHT.px;
            }
        };
        let dock = el! {
            .div {
                # "files-terminal-dock"; key_context: TERMINAL_CONTEXT; track_focus: $&self.terminal.focus;
                on_action: $(window.listener_for(&panel, Panel::on_new));
                on_action: $(window.listener_for(&panel, Panel::on_close));
                test_support; flex; flex_col; flex_1; min_h_0; min_w_0; overflow_hidden;
            } [
                header,
                .div { flex; flex_col; flex_1; min_h_0; min_w_0; overflow_hidden; } [panel],
            ]
        };
        let divider = el! {
            $(frame.workspace(el! { .div }.into_any_element(), el! { .div }.into_any_element(), cx)) {
                flex_none; size_full;
            }
        };
        let top = if progress == 1. && maximized_progress == 1. {
            el! { .div }.into_any_element()
        } else {
            workspace
        };
        let bottom = if progress > 0. {
            dock.into_any_element()
        } else {
            el! { .div }.into_any_element()
        };
        el! { $(self.terminal.split.clone()).VerticalSplit::new {
            top: top; bottom_progress: progress; bottom_maximized_progress: maximized_progress;
            bottom: bottom; label: $(i18n::text("files-terminal-resize", cx)); divider_background: divider;
        } }.into_any_element()
    }

    pub(in crate::pages::workspace) fn on_terminal_split(
        &mut self,
        _: Entity<ui::split::State>,
        event: &ui::split::Changed,
        cx: &mut Context<Self>,
    ) {
        self.edit_preferences(
            |preferences| {
                preferences.files.terminal_content_height = Some(event.0);
            },
            cx,
        );
    }

    pub(in crate::pages::workspace) fn apply_terminal_preferences(&self, cx: &mut Context<Self>) {
        if let Some(dock) = &self.terminal.dock {
            let panel = dock.panel.clone();
            panel.update(cx, |panel, cx| {
                panel.set_preferences(self.settings.preferences.terminal.clone(), cx)
            });
        }
    }

    fn closing_panels(&self) -> Vec<Entity<Panel>> {
        self.terminal
            .dock
            .iter()
            .map(|dock| dock.panel.clone())
            .collect()
    }

    pub(in crate::pages::workspace) fn on_close_window(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Client-side and WM close requests must both stop the window's PTYs.
        self.request_terminal_close(window, cx);
    }

    pub(in crate::pages::workspace) fn install_terminal_close_handler(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            owner
                .update(cx, |this, cx| {
                    if this.terminal.lifecycle == Lifecycle::Closing {
                        return false;
                    }
                    if this.closing_panels().is_empty() {
                        return true;
                    }
                    this.request_terminal_close(window, cx);
                    false
                })
                .unwrap_or(true)
        });
    }

    pub(in crate::pages::workspace) fn request_terminal_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal.lifecycle == Lifecycle::Closing {
            return;
        }
        // Each terminal registers its cleanup with the application before the
        // window disappears, including launches that have not finished yet.
        self.terminal.lifecycle = Lifecycle::Closing;
        for panel in self.closing_panels() {
            drop(panel.update(cx, |panel, cx| panel.shutdown(cx)));
        }
        window.remove_window();
    }
}
