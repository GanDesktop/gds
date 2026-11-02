//! The Files window owns terminal sessions independently of file locations.

use gds::appearance::tokens;
mod panel;

use super::*;
use gpui_kit::{assets::IconName, component::ActiveTheme, prelude::*};
use panel::Panel;

#[cfg(test)]
mod tests;

// Keep a tab, the optional volume group, and at least one location reachable.
pub(super) const MIN_CONTENT_HEIGHT: f32 = sidebar::MIN_TABS_HEIGHT
    + sidebar::MIN_DEVICES_HEIGHT
    + ui::split::DIVIDER_HEIGHT
    + tokens::SIDEBAR_ITEM_HEIGHT * 2.;
pub(super) const MIN_DOCK_HEIGHT: f32 = 160.;
pub(super) const DEFAULT_DOCK_HEIGHT: f32 = 280.;
const COLLAPSED_ACTION_INSET: f32 =
    (tokens::COLLAPSED_SIDEBAR_WIDTH - 2. * tokens::SIDEBAR_PADDING - tokens::ICON_BUTTON_SIZE)
        / 2.;

/// Sessions and selection belong to this window, never a file location.
pub(super) struct State {
    pub dock: Option<Dock>,
    focus: FocusHandle,
    pub visible: bool,
    pub maximized: bool,
    pub split: Entity<ui::split::State>,
    pub close_pending: Option<CloseTarget>,
    pub closing: bool,
}

impl State {
    pub(super) fn new(preferences: &configuration::Preferences, cx: &mut App) -> Self {
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
            close_pending: None,
            closing: false,
        }
    }
}

pub(super) struct Dock {
    pub panel: Entity<Panel>,
    cwd: Option<PathBuf>,
    _changes: Subscription,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CloseTarget {
    Window,
}

/// Only a real available directory is a local launch target.
fn local_cwd(path: &Path, available: bool) -> Option<PathBuf> {
    (available && path.is_absolute() && path.is_dir()).then(|| path.to_path_buf())
}

fn launch_cwd(path: Option<&Path>, available: bool, home: &Path) -> PathBuf {
    path.and_then(|path| local_cwd(path, available))
        .unwrap_or_else(|| home.to_path_buf())
}

impl Browser {
    pub(super) fn terminal_visible(&self) -> bool {
        self.settings.preferences.terminal_enabled()
            && self.terminal.visible
            && self.picker.is_none()
            && self.pages.is_empty()
    }

    pub(super) fn terminal_focused(&self, window: &Window, cx: &App) -> bool {
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
        if self.terminal.closing {
            return None;
        }
        if let Some(dock) = &self.terminal.dock {
            return Some(dock.panel.clone());
        }
        let cwd = self.terminal_cwd(cx);
        let preferences = self.settings.preferences.terminal.clone();
        let panel = cx.new(|cx| {
            let mut panel = Panel::new(cx);
            panel.set_preferences(preferences, cx);
            panel.set_cwd(cwd.clone(), cx);
            panel
        });
        let changes = cx.observe(&panel, |_, _, cx| cx.notify());
        self.terminal.dock = Some(Dock {
            panel: panel.clone(),
            cwd,
            _changes: changes,
        });
        Some(panel)
    }

    pub(super) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.ensure_terminal(cx) else {
            return;
        };
        self.pages.clear();

        self.toolbar.path.editing = false;
        self.terminal.visible = !self.terminal.visible;
        if self.terminal.visible {
            panel.update(cx, |panel, cx| panel.reveal(window, cx));
        } else {
            self.window.focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn open_internal_terminal(
        &mut self,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.picker.is_some() || !self.pages.is_empty() {
            return;
        }
        let available = self.active().is_some_and(|slot| slot.files_available())
            && self.device_path_available(&cwd, cx);
        let cwd = launch_cwd(Some(&cwd), available, &self.locations.home);
        let Some(panel) = self.ensure_terminal(cx) else {
            return;
        };

        self.toolbar.path.editing = false;
        self.terminal.visible = true;
        // Each request starts a fresh session; existing PTYs are never redirected.
        panel.update(cx, |panel, cx| panel.open(cwd, window, cx));
        cx.notify();
    }

    /// File navigation only changes the *next* new-session target. It never
    /// writes a command into the active PTY, even when the shell looks idle.
    pub(super) fn sync_terminal_cwd(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_visible()
            && let Some(panel) = self.ensure_terminal(cx)
            && panel.read(cx).needs_initial_reveal()
        {
            // Rendering only prepares the window panel. Defer PTY startup and
            // recheck visibility so hiding the dock cancels initial startup.
            let owner = cx.entity().downgrade();
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
        }
        let cwd = self.terminal_cwd(cx);
        let Some(dock) = self.terminal.dock.as_mut() else {
            return;
        };
        if dock.cwd != cwd {
            dock.cwd = cwd.clone();
            dock.panel.update(cx, |panel, cx| panel.set_cwd(cwd, cx));
        }
    }

    pub(super) fn on_return_from_terminal(
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

    pub(super) fn terminal_workspace(
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
            return ui::split::VerticalSplit::new(self.terminal.split.clone())
                .top(workspace)
                .bottom_progress(0.)
                .into_any_element();
        };
        let panel = dock.panel.clone();
        let maximized = self.terminal.maximized;
        let maximize = ui::IconButton::new("files-terminal-maximize")
            .icon(if maximized {
                IconName::Minimize2
            } else {
                IconName::Maximize2
            })
            .label(i18n::text(
                if maximized && self.terminal_visible() {
                    "files-terminal-restore"
                } else {
                    "files-terminal-maximize"
                },
                cx,
            ))
            .on_click(cx.listener(Self::maximize_terminal));
        let width = self.width();
        panel.update(cx, |panel, cx| {
            panel.set_sidebar_presentation(self.sidebar.presentation, cx)
        });
        let frame = ui::Frame::default().sidebar_width(width);
        let title = gds::i18n::t!(@cx, "settings-terminal");
        let heading = div()
            .id("files-terminal-title")
            .flex()
            .items_center()
            .h_full()
            .min_w_0()
            .px(px(tokens::SIDEBAR_PADDING))
            .text_color(cx.theme().muted_foreground)
            .pr(px(tokens::SIDEBAR_PADDING
                + self
                    .sidebar
                    .presentation
                    .interpolate(0., COLLAPSED_ACTION_INSET)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .opacity(self.sidebar.presentation.label_opacity())
                    .child(
                        ui::sidebar::SideBarSectionTitle::new(title)
                            .px(px(tokens::SIDEBAR_SECTION_PADDING)),
                    ),
            )
            .child(panel.update(cx, |panel, cx| panel.new_button(cx)));
        let active_title = panel.read(cx).active_title(cx);
        let title_hint = panel
            .read(cx)
            .active_hint(cx)
            .unwrap_or_else(|| ui::tooltip::Details::new(active_title.clone()));
        let controls = div()
            .flex()
            .items_center()
            .h_full()
            .min_w_0()
            .px(px(tokens::TERMINAL_CONTENT_INSET))
            .gap_1()
            .child(
                div()
                    .id("files-active-terminal-title")
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(tokens::SMALL)
                    .hoverable_tooltip(move |window, cx| title_hint.view(window, cx))
                    .child(active_title),
            )
            .child(
                ui::IconButton::new("files-terminal-switch-directory")
                    .icon(IconName::FolderInput)
                    .label(i18n::text("files-terminal-switch-directory", cx))
                    .disabled(
                        self.collection().is_some()
                            || self.terminal_cwd(cx).is_none()
                            || !panel.read(cx).can_change_directory(cx),
                    )
                    .on_click(cx.listener(Self::switch_terminal_directory)),
            )
            .child(maximize);
        let header = frame
            .workspace(heading.into_any_element(), controls.into_any_element(), cx)
            .flex_none()
            .h(px(tokens::TERMINAL_HEADER_HEIGHT));
        let dock = div()
            .id("files-terminal-dock")
            .key_context(TERMINAL_CONTEXT)
            .track_focus(&self.terminal.focus)
            .on_action(window.listener_for(&panel, Panel::on_new))
            .on_action(window.listener_for(&panel, Panel::on_close))
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(panel),
            );
        let divider = frame
            .workspace(div().into_any_element(), div().into_any_element(), cx)
            .flex_none()
            .size_full();
        ui::split::VerticalSplit::new(self.terminal.split.clone())
            .top(if progress == 1. && maximized_progress == 1. {
                div().into_any_element()
            } else {
                workspace
            })
            .bottom_progress(progress)
            .bottom_maximized_progress(maximized_progress)
            .bottom(if progress > 0. {
                dock.into_any_element()
            } else {
                div().into_any_element()
            })
            .label(i18n::text("files-terminal-resize", cx))
            .divider_background(divider)
            .into_any_element()
    }

    pub(super) fn on_terminal_split(
        &mut self,
        _: Entity<ui::split::State>,
        event: &ui::split::Changed,
        cx: &mut Context<Self>,
    ) {
        self.settings.preferences.files.terminal_content_height = Some(event.0);
        if let Err(error) = configuration::save(&self.settings.preferences) {
            report_settings_failure("settings-save-failed", &error, cx);
        }
    }

    pub(super) fn apply_terminal_preferences(&self, cx: &mut Context<Self>) {
        if let Some(dock) = &self.terminal.dock {
            let panel = dock.panel.clone();
            panel.update(cx, |panel, cx| {
                panel.set_preferences(self.settings.preferences.terminal.clone(), cx)
            });
        }
    }

    fn closing_panels(&self, _: CloseTarget) -> Vec<Entity<Panel>> {
        self.terminal
            .dock
            .iter()
            .map(|dock| dock.panel.clone())
            .collect()
    }

    pub(super) fn on_close_window(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Client-side titlebar controls remove windows directly; route them
        // through the same shutdown barrier as WM requests.
        self.request_terminal_close(CloseTarget::Window, window, cx);
    }

    pub(super) fn install_terminal_close_handler(window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            owner
                .update(cx, |this, cx| {
                    if this.terminal.close_pending.is_some() {
                        return false;
                    }
                    if this.closing_panels(CloseTarget::Window).is_empty() {
                        return true;
                    }
                    this.request_terminal_close(CloseTarget::Window, window, cx);
                    false
                })
                .unwrap_or(true)
        });
    }

    pub(super) fn request_terminal_close(
        &mut self,
        target: CloseTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal.close_pending.is_some() {
            return;
        }
        self.finish_terminal_close(target, window, cx);
    }

    fn finish_terminal_close(
        &mut self,
        target: CloseTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal.closing {
            return;
        }
        // Shutdown futures include in-flight spawns and worker cleanup, so the
        // final window cannot quit before the last PTY has actually stopped.
        self.terminal.closing = true;
        self.terminal.close_pending = Some(target);
        let shutdowns: Vec<_> = self
            .closing_panels(target)
            .into_iter()
            .map(|panel| panel.update(cx, |panel, cx| panel.shutdown(cx)))
            .collect();
        cx.spawn_in(window, async move |this, cx| {
            // Every shutdown already requested termination before waiting.
            for shutdown in shutdowns {
                shutdown.await;
            }
            if let Err(error) = this.update_in(cx, |this, window, _cx| {
                this.terminal.closing = false;
                this.terminal.close_pending = None;
                match target {
                    CloseTarget::Window => window.remove_window(),
                }
            }) {
                tracing::warn!("Terminal shutdown finished after the Files window closed: {error}");
            }
        })
        .detach();
    }
}
