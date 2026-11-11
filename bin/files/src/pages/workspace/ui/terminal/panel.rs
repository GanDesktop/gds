//! Files' window-level terminal collection, selection and tab-bar interactions.
//! Process startup and shutdown races belong to the single shared Terminal.

#[cfg(all(test, feature = "gui-tests", unix))]
mod tests;

use gds::appearance::tokens;
use gds::ui::DialogActions;
use gds::ui::IconButton;
use gds::ui::NavItem;
use gds::ui::Shortcut;
use gds::ui::button::Button;
use std::path::PathBuf;

use super::super::drag_drop::FileDrag;
use gds_uim::el;

use gds::{
    i18n::{self, t},
    ui,
    ui::terminal::{self, Terminal},
};
use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme, Disableable, Icon, Selectable, WindowExt},
    prelude::*,
    *,
};

use crate::shared::ui::actions::{
    CloseTerminal, FocusTerminal, NewTerminal, NextTerminal, PreviousTerminal, RenameTerminal,
    TERMINAL_TABS_CONTEXT,
};

struct Entry {
    id: u64,
    terminal: Entity<Terminal>,
    custom_name: Option<SharedString>,
    closing: bool,
    _subscription: Subscription,
}

pub(in crate::pages::workspace) struct Panel {
    entries: Vec<Entry>,
    active: Option<u64>,
    next_id: u64,
    tabs_focus: FocusHandle,
    cwd: Option<PathBuf>,
    preferences: terminal::Preferences,
    sidebar: ui::sidebar::SidebarPresentation,
    tabs_scroll: ScrollHandle,
    sealed: bool,
}

impl Panel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            entries: Vec::new(),
            active: None,
            next_id: 0,
            tabs_focus: cx.focus_handle(),
            cwd: None,
            preferences: terminal::Preferences::default(),
            sidebar: Default::default(),
            tabs_scroll: ScrollHandle::new(),
            sealed: false,
        }
    }

    pub fn set_cwd(&mut self, cwd: Option<PathBuf>, cx: &mut Context<Self>) {
        let cwd = cwd.filter(|path| path.is_absolute());
        if self.cwd != cwd {
            self.cwd = cwd;
            cx.notify();
        }
    }

    pub fn set_sidebar_presentation(
        &mut self,
        sidebar: ui::sidebar::SidebarPresentation,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar != sidebar {
            self.sidebar = sidebar;
            cx.notify();
        }
    }

    pub fn set_preferences(&mut self, preferences: terminal::Preferences, cx: &mut Context<Self>) {
        if self.preferences == preferences {
            return;
        }
        self.preferences = preferences;
        for entry in &self.entries {
            entry.terminal.update(cx, |terminal, cx| {
                terminal.set_preferences(self.preferences.clone(), cx)
            });
        }
        cx.notify();
    }

    pub fn open(&mut self, cwd: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.sealed || !cwd.is_absolute() {
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let preferences = self.preferences.clone();
        let terminal = cx.new(|cx| Terminal::new(cwd, preferences, window, cx));
        let subscription = cx.subscribe(&terminal, |_, _, _: &terminal::Event, cx| cx.notify());
        self.entries.push(Entry {
            id,
            terminal: terminal.clone(),
            custom_name: None,
            closing: false,
            _subscription: subscription,
        });
        self.active = Some(id);
        terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
        cx.notify();
    }

    pub fn needs_initial_reveal(&self) -> bool {
        !self.sealed && self.next_id == 0 && self.cwd.is_some()
    }

    /// Opening an empty dock starts once; redraws and closing its last tab do not.
    pub fn reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sealed {
            return;
        }
        if self.entries.is_empty()
            && let Some(cwd) = self.cwd.clone()
        {
            self.open(cwd, window, cx);
        } else {
            self.focus(window, cx);
        }
    }

    fn selected(&self) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|entry| Some(entry.id) == self.active)
    }

    fn title(entry: &Entry, cx: &App) -> SharedString {
        if let Some(name) = &entry.custom_name {
            return name.clone();
        }
        let terminal = entry.terminal.read(cx);
        terminal
            .title(cx)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| {
                terminal
                    .cwd()
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| terminal.cwd().display().to_string())
                    .into()
            })
    }

    fn hint(entry: &Entry, cx: &App) -> ui::tooltip::Details {
        let terminal = entry.terminal.read(cx);
        let origin = t!(
            @cx,
            "files-terminal-started-in",
            path = terminal.cwd().display().to_string()
        );
        let status = match terminal.status(cx) {
            terminal::Status::Starting => i18n::text("gds-terminal-starting", cx),
            terminal::Status::Running => i18n::text("gds-terminal-running", cx),
            terminal::Status::Stopping => i18n::text("gds-terminal-stopping", cx),
            terminal::Status::Closed => i18n::text("gds-terminal-exited", cx),
            terminal::Status::Exited { code, signal } => t!(
                @cx,
                "gds-terminal-exit-status",
                code = i64::from(code),
                signal = signal.unwrap_or_else(|| "—".into())
            ),
            terminal::Status::Failed(error) => error.into(),
        };
        ui::tooltip::Details::new(Self::title(entry, cx))
            .line(origin)
            .line(status)
    }

    pub fn active_hint(&self, cx: &App) -> Option<ui::tooltip::Details> {
        self.selected().map(|entry| Self::hint(entry, cx))
    }

    pub fn can_change_directory(&self, cx: &App) -> bool {
        !self.sealed
            && self.selected().is_some_and(|entry| {
                !entry.closing && entry.terminal.read(cx).status(cx) == terminal::Status::Running
            })
    }

    pub fn change_directory(&mut self, cwd: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_change_directory(cx) || window.has_active_dialog(cx) {
            return;
        }
        // Never inject a cd/Enter into the selected shell: its unconfirmed
        // editing buffer is not observable. Preserve that session and start a
        // fresh tab with the requested working directory instead.
        self.open(cwd, window, cx);
    }

    pub fn active_title(&self, cx: &App) -> SharedString {
        self.selected()
            .map(|entry| Self::title(entry, cx))
            .unwrap_or_else(|| i18n::text("settings-terminal", cx))
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        if let Some(entry) = self.selected().filter(|entry| !entry.closing) {
            entry
                .terminal
                .update(cx, |terminal, cx| terminal.focus(window, cx));
        } else {
            self.tabs_focus.focus(window, cx);
        }
    }

    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.tabs_focus.contains_focused(window, cx)
            || self
                .entries
                .iter()
                .any(|entry| entry.terminal.read(cx).contains_focus(window, cx))
    }

    pub fn shutdown(&mut self, cx: &mut Context<Self>) -> terminal::Shutdown {
        self.sealed = true;
        let shutdowns: Vec<_> = self
            .entries
            .iter_mut()
            .map(|entry| {
                entry.closing = true;
                entry
                    .terminal
                    .update(cx, |terminal, cx| terminal.shutdown(cx))
            })
            .collect();
        cx.notify();
        // Closing entries remain owned until their cleanup completes. Repeated
        // collection shutdowns therefore wait on the same single-terminal work.
        Box::pin(async move {
            for shutdown in shutdowns {
                shutdown.await;
            }
        })
    }

    fn paste_paths(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.selected() {
            let terminal = entry.terminal.clone();
            terminal.update(cx, |terminal, cx| terminal.paste_paths(paths, window, cx));
        }
    }

    pub(super) fn on_new(&mut self, _: &NewTerminal, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx)
            && let Some(cwd) = self.cwd.clone()
        {
            self.open(cwd, window, cx);
        }
    }

    pub fn new_button(&self, panel: &Entity<Self>, window: &mut Window, cx: &App) -> IconButton {
        let on_click = window.listener_for(panel, |panel, _, window, cx| {
            panel.on_new(&NewTerminal, window, cx)
        });
        el! { "files-terminal-new".IconButton::new {
            icon: IconName::Plus;
            label: $(i18n::text("files-terminal-new", cx));
            disabled: $(self.cwd.is_none() || self.sealed);
            on_click: on_click;
        } }
    }

    fn rename_entry(&mut self, id: u64, name: &str, cx: &mut Context<Self>) {
        if self.sealed {
            return;
        }
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == id && !entry.closing)
        {
            entry.custom_name = (!name.trim().is_empty()).then(|| name.trim().to_owned().into());
            cx.notify();
        }
    }

    fn on_rename(&mut self, _: &RenameTerminal, window: &mut Window, cx: &mut Context<Self>) {
        if self.sealed || window.has_active_dialog(cx) {
            return;
        }
        let Some(entry) = self.selected().filter(|entry| !entry.closing) else {
            return;
        };
        let id = entry.id;
        let name = Self::title(entry, cx);
        let form = cx.new(|cx| {
            super::super::name_form::NameForm::new(
                name.to_string().into(),
                i18n::text("files-terminal-rename", cx),
                window,
                cx,
            )
        });
        let panel = cx.entity().downgrade();
        let focus_form = form.clone();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let panel = panel.clone();
            let form = form.clone();
            let content = form.clone();
            let footer = el! { .DialogActions::default {
                primary_label: $(i18n::text("rename-confirm", cx));
                primary_icon: IconName::Pencil;
                cancel_label: $(i18n::text("operation-cancel", cx));
            } };
            el! {
                dialog {
                    confirm; width: 480.0.px;
                    bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                    title: $(i18n::text("files-terminal-rename", cx));
                    icon: IconName::TextCursor.Icon::new;
                    footer: footer;
                    on_ok: $(move |_, _, cx| {
                        let name = form.read(cx).chosen_name(cx).to_string_lossy().into_owned();
                        if let Err(error) = panel.update(cx, |panel, cx| panel.rename_entry(id, &name, cx)) {
                            tracing::warn!("Terminal rename dialog outlived its panel: {error}");
                        }
                        true
                    });
                } [content]
            }
        });
        window.on_next_frame(move |window, cx| {
            focus_form.update(cx, |form, cx| form.focus_input(window, cx));
        });
    }

    pub(super) fn on_close(
        &mut self,
        _: &CloseTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        if let Some(id) = self.active {
            self.request_close(id, window, cx);
        }
    }

    fn on_focus(&mut self, _: &FocusTerminal, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx) {
            self.focus(window, cx);
        }
    }

    fn on_next(&mut self, _: &NextTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle(false, window, cx);
    }

    fn on_previous(&mut self, _: &PreviousTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle(true, window, cx);
    }

    fn cycle(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.sealed || self.entries.is_empty() || window.has_active_dialog(cx) {
            return;
        }
        let current = self
            .entries
            .iter()
            .position(|entry| Some(entry.id) == self.active)
            .unwrap_or(0);
        let count = self.entries.len();
        let next = if backwards {
            (current + count - 1) % count
        } else {
            (current + 1) % count
        };
        self.select(self.entries[next].id, window, cx);
    }

    fn select(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.sealed || !self.entries.iter().any(|entry| entry.id == id) {
            return;
        }
        self.active = Some(id);
        self.tabs_focus.focus(window, cx);
        cx.notify();
    }

    fn request_close(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.close_approved(id, window, cx);
    }

    fn close_approved(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.sealed {
            return;
        }
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == id && !entry.closing)
        else {
            return;
        };
        entry.closing = true;
        drop(
            entry
                .terminal
                .update(cx, |terminal, cx| terminal.shutdown(cx)),
        );
        self.remove_entry(id, window, cx);
    }

    fn tab(&self, entry: &Entry, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let id = entry.id;
        let selected = self.active == Some(id);
        let shortcut_target = selected && self.contains_focus(window, cx);
        let group = SharedString::from(format!("files-terminal-row-{id}"));
        let hint = Self::hint(entry, cx);
        let close_label = i18n::text("files-terminal-end", cx);
        let compact = self.sidebar.progress() == 0.;
        let disabled = self.sealed || entry.closing;
        let on_close = cx.listener(move |panel, _, window, cx| {
            cx.stop_propagation();
            panel.request_close(id, window, cx);
        });
        let close_shortcut = Shortcut::new(Box::new(CloseTerminal), Some(TERMINAL_TABS_CONTEXT));
        let close = if compact {
            let button = gds::ui::corner_button(
                ("files-terminal-close", id),
                IconName::X,
                close_label.clone(),
            );
            el! {
                $(button) {
                    accessibility_label: $(close_label.clone());
                    selected: selected;
                    @if (shortcut_target)
                        tooltip_with_shortcut: $(close_label.clone()), close_shortcut;
                    @else
                        tooltip: close_label;
                    @end;
                    disabled: disabled;
                    on_click: on_close;
                }
            }
        } else {
            el! { $(("files-terminal-close", id)).Button::icon_button {
                control_icon: IconName::X;
                accessibility_label: $(close_label.clone());
                selected: selected;
                @if (shortcut_target)
                    tooltip_with_shortcut: $(close_label.clone()), close_shortcut;
                @else
                    tooltip: close_label;
                @end;
                disabled: disabled;
                on_click: on_close;
            } }
        };
        let reveal_close = shortcut_target
            && ui::HeldModifier::from_modifiers(window.modifiers()) == Some(ui::HeldModifier::Ctrl);
        let mut trailing = Vec::new();
        let mut corner = None;
        if compact {
            corner = Some(gds::ui::corner_action(group.clone(), close, reveal_close));
        } else {
            let close = close.into_any_element();
            let close = if reveal_close {
                close
            } else {
                gds::ui::hover_action(group.clone(), el! { .div }.into_any_element(), Some(close))
            };
            trailing.push(close);
        }
        let on_select = cx.listener(move |panel, _, window, cx| panel.select(id, window, cx));
        let tooltip = move |window: &mut Window, cx: &mut App| hint.view(window, cx);
        el! {
            .div {
                # $("files-terminal-row", id); test_support; relative; group: group;
                w_full; flex_shrink_0; tooltip_show_delay: tokens::TOOLTIP_DELAY;
                hoverable_tooltip: tooltip;
            } [
                ($("files-terminal-tab", id), IconName::Terminal, $(Self::title(entry, cx))).NavItem::new {
                    presentation: $(self.sidebar); selected: selected; tooltip_enabled: false;
                    trailing: trailing; on_click: on_select;
                },
                ..corner,
            ]
        }.into_any_element()
    }

    fn remove_entry(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.sealed {
            return;
        }
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return;
        };
        let focused = self.contains_focus(window, cx);
        let was_active = self.active == Some(id);
        self.entries.remove(index);
        if was_active {
            self.active = self
                .entries
                .get(index.min(self.entries.len().saturating_sub(1)))
                .map(|entry| entry.id);
            if focused {
                self.tabs_focus.focus(window, cx);
            }
        }
        cx.notify();
    }
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = gds::appearance::theme::Colors::current(cx);
        let tabs = el! {
            .div {
                # "files-terminal-tabs"; test_support; w: $(self.sidebar.width()).px;
                h_full; flex_shrink_0; flex; flex_col; overflow_y_scroll;
                track_scroll: $&self.tabs_scroll; p: tokens::SIDEBAR_PADDING.px; gap_1;
                bg: $(gds::appearance::theme::sidebar_background(cx));
                text_color: $(colors.sidebar_foreground); key_context: TERMINAL_TABS_CONTEXT;
                track_focus: $&self.tabs_focus;
                on_action: $(cx.listener(Self::on_next));
                on_action: $(cx.listener(Self::on_previous));
                on_action: $(cx.listener(Self::on_focus));
            } [..$(self.entries.iter().map(|entry| self.tab(entry, window, cx)))]
        };
        let body = match self.selected() {
            Some(entry) => entry.terminal.clone().into_any_element(),
            None => el! {
                .div {
                    size_full; p_3; bg: $(gds::appearance::theme::content_background(cx));
                    text_color: $(colors.muted_foreground);
                } [$(i18n::text(if self.cwd.is_some() {
                    "files-terminal-empty"
                } else {
                    "files-terminal-cwd-unavailable"
                }, cx))]
            }
            .into_any_element(),
        };
        let on_file_drop = cx.listener(|panel, files: &FileDrag, window, cx| {
            panel.paste_paths(&files.paths(), window, cx);
        });
        let on_external_drop = cx.listener(|panel, files: &ExternalPaths, window, cx| {
            panel.paste_paths(files.paths(), window, cx);
        });
        el! {
            .div {
                on_modifiers_changed: $(cx.listener(|_, _, _, cx| cx.notify()));
                on_action: $(cx.listener(Self::on_new));
                on_action: $(cx.listener(Self::on_close));
                on_action: $(cx.listener(Self::on_rename));
                relative; size_full; min_h_0; min_w_0; flex; overflow_hidden;
            } [
                tabs,
                .div {
                    # "files-terminal-input"; test_support; flex_1; min_h_0; min_w_0; h_full;
                    on_drop: on_file_drop; on_drop: on_external_drop;
                    drag_over: $(|style, _: &FileDrag, _, cx| style.bg(cx.theme().drop_target));
                    drag_over: $(|style, _: &ExternalPaths, _, cx| style.bg(cx.theme().drop_target));
                } [body],
                .div {
                    # "files-terminal-divider"; test_support; absolute;
                    left: $(self.sidebar.width() - 1.).px; top_0; bottom_0; w: 1.0.px;
                    bg: $(colors.border);
                },
            ]
        }
    }
}
