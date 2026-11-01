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
        self.terminal_visible && self.picker.is_none() && !self.settings_open
    }

    pub(super) fn terminal_focused(&self, window: &Window, cx: &App) -> bool {
        self.terminal
            .as_ref()
            .is_some_and(|dock| dock.panel.read(cx).contains_focus(window, cx))
    }

    fn terminal_cwd(&self, cx: &App) -> Option<PathBuf> {
        if self.picker.is_some() {
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
            &self.home,
        ))
    }

    /// Creating the window's empty panel does not start a shell.
    fn ensure_terminal(&mut self, cx: &mut Context<Self>) -> Option<Entity<Panel>> {
        if self.picker.is_some() {
            return None;
        }
        if self.terminal_closing {
            return None;
        }
        if let Some(dock) = &self.terminal {
            return Some(dock.panel.clone());
        }
        let cwd = self.terminal_cwd(cx);
        let preferences = self.preferences.terminal.clone();
        let panel = cx.new(|cx| {
            let mut panel = Panel::new(cx);
            panel.set_preferences(preferences, cx);
            panel.set_cwd(cwd.clone(), cx);
            panel
        });
        let changes = cx.observe(&panel, |_, _, cx| cx.notify());
        self.terminal = Some(Dock {
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
        self.settings_open = false;
        self.reset_tab_prefix();
        self.path_editing = false;
        self.terminal_visible = !self.terminal_visible;
        if self.terminal_visible {
            panel.update(cx, |panel, cx| panel.reveal(window, cx));
        } else {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn open_internal_terminal(
        &mut self,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.picker.is_some() || self.settings_open {
            return;
        }
        let available = self.active().is_some_and(|slot| slot.files_available())
            && self.device_path_available(&cwd, cx);
        let cwd = launch_cwd(Some(&cwd), available, &self.home);
        let Some(panel) = self.ensure_terminal(cx) else {
            return;
        };
        self.reset_tab_prefix();
        self.path_editing = false;
        self.terminal_visible = true;
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
        let Some(dock) = self.terminal.as_mut() else {
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
        if window.has_active_dialog(cx) || self.rename.read(cx).is_open() {
            return;
        }
        if !self.terminal_focused(window, cx) || self.settings_open {
            cx.propagate();
            return;
        }
        self.restore_terminal_content(window, cx);
    }

    fn restore_terminal_content(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_maximized = false;
        self.focus.focus(window, cx);
        Self::restore_file_focus(cx.entity().downgrade(), self.active_id, window);
        cx.notify();
    }

    fn switch_terminal_directory(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_open || self.collection().is_some() || window.has_active_dialog(cx) {
            return;
        }
        let Some(cwd) = self.terminal_cwd(cx) else {
            notifications::info(i18n::text("files-terminal-cwd-unavailable", cx), cx);
            return;
        };
        // Do not ensure/reveal here: this operation only targets an existing selection.
        let Some(dock) = self.terminal.as_ref() else {
            return;
        };
        dock.panel
            .update(cx, |panel, cx| panel.change_directory(cwd, window, cx));
    }

    fn maximize_terminal(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dock) = self.terminal.as_ref() else {
            return;
        };
        let panel = dock.panel.clone();
        self.terminal_maximized = !self.terminal_maximized;
        panel.update(cx, |panel, cx| panel.focus(window, cx));
        cx.notify();
    }

    pub(super) fn terminal_workspace(
        &self,
        workspace: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.terminal_visible() {
            return workspace;
        }
        let Some(dock) = self.terminal.as_ref() else {
            return workspace;
        };
        let panel = dock.panel.clone();
        let maximized = self.terminal_maximized;
        let maximize = ui::IconButton::new("files-terminal-maximize")
            .icon(if maximized {
                IconName::Minimize2
            } else {
                IconName::Maximize2
            })
            .label(i18n::text(
                if maximized {
                    "files-terminal-restore"
                } else {
                    "files-terminal-maximize"
                },
                cx,
            ))
            .on_click(cx.listener(Self::maximize_terminal));
        let width = self.width();
        panel.update(cx, |panel, cx| panel.set_sidebar_width(width, cx));
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
            .when(!self.sidebar_collapsed, |this| {
                this.child(
                    ui::sidebar::SideBarSectionTitle::new(title)
                        .flex_1()
                        .px(px(tokens::SIDEBAR_SECTION_PADDING)),
                )
            })
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
        if maximized {
            dock.into_any_element()
        } else {
            let divider = frame
                .workspace(div().into_any_element(), div().into_any_element(), cx)
                .flex_none()
                .size_full();
            ui::split::VerticalSplit::new(self.terminal_split.clone())
                .top(workspace)
                .bottom(dock)
                .label(i18n::text("files-terminal-resize", cx))
                .divider_background(divider)
                .into_any_element()
        }
    }

    pub(super) fn on_terminal_split(
        &mut self,
        _: Entity<ui::split::State>,
        event: &ui::split::Changed,
        cx: &mut Context<Self>,
    ) {
        self.preferences.files.terminal_content_height = Some(event.0);
        if let Err(error) = configuration::save(&self.preferences) {
            report_settings_failure("settings-save-failed", &error, cx);
        }
    }

    pub(super) fn apply_terminal_preferences(&self, cx: &mut Context<Self>) {
        if let Some(dock) = &self.terminal {
            let panel = dock.panel.clone();
            panel.update(cx, |panel, cx| {
                panel.set_preferences(self.preferences.terminal.clone(), cx)
            });
        }
    }

    fn closing_panels(&self, _: CloseTarget) -> Vec<Entity<Panel>> {
        self.terminal
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
                    if this.terminal_close_pending.is_some() {
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
        if self.terminal_close_pending.is_some() {
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
        if self.terminal_closing {
            return;
        }
        // Shutdown futures include in-flight spawns and worker cleanup, so the
        // final window cannot quit before the last PTY has actually stopped.
        self.terminal_closing = true;
        self.terminal_close_pending = Some(target);
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
                this.terminal_closing = false;
                this.terminal_close_pending = None;
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
