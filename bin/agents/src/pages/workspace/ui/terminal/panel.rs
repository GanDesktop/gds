//! Agents owns terminal selection, management shortcuts and close policy.

use crate::shared::i18n::tr;
use futures::{FutureExt as _, future::Shared};
use gds::ui::terminal::{Preferences, Shutdown, Status, Terminal};
use gpui_kit::{
    App, AppContext, Context, Entity, FocusHandle, IntoElement, Render, RenderOnce, SharedString,
    Subscription, TestSupportExt, Window,
    assets::IconName,
    component::{ActiveTheme, Selectable},
    div,
    prelude::*,
};
use std::path::PathBuf;

use super::{
    CloseTerminal, FocusTerminal, NewTerminal, NextTerminal, PreviousTerminal, TABS_CONTEXT,
};
use crate::shared::ui::Button;

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

struct Entry {
    id: u64,
    terminal: Entity<Terminal>,
    _events: Subscription,
}

pub(in crate::pages::workspace) struct Panel {
    entries: Vec<Entry>,
    active: Option<u64>,
    next_id: u64,
    cwd: Option<PathBuf>,
    preferences: Preferences,
    tabs_focus: FocusHandle,
    empty_focus: FocusHandle,
    closing: bool,
    retiring: Vec<Shared<Shutdown>>,
    shutdown: Option<Shared<Shutdown>>,
}

impl Panel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            entries: Vec::new(),
            active: None,
            next_id: 0,
            cwd: None,
            preferences: Preferences::default(),
            tabs_focus: cx.focus_handle(),
            empty_focus: cx.focus_handle(),
            closing: false,
            retiring: Vec::new(),
            shutdown: None,
        }
    }

    pub fn set_cwd(&mut self, cwd: Option<PathBuf>, cx: &mut Context<Self>) {
        let cwd = cwd.filter(|path| path.is_absolute());
        if self.cwd != cwd {
            self.cwd = cwd;
            cx.notify();
        }
    }

    pub fn set_preferences(&mut self, preferences: Preferences, cx: &mut Context<Self>) {
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
        if self.closing || !cwd.is_absolute() {
            return;
        }
        let terminal = cx.new(|cx| Terminal::new(cwd, self.preferences.clone(), window, cx));
        let events = cx.subscribe(&terminal, |_, _, _: &gds::ui::terminal::Event, cx| {
            cx.notify()
        });
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(Entry {
            id,
            terminal: terminal.clone(),
            _events: events,
        });
        self.active = Some(id);
        terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
        cx.notify();
    }

    pub fn needs_initial_reveal(&self) -> bool {
        !self.closing && self.next_id == 0 && self.cwd.is_some()
    }

    /// Opening an empty dock starts once; redraws and closing its last tab do not.
    pub fn reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
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

    fn active_terminal(&self) -> Option<&Entity<Terminal>> {
        self.entries
            .iter()
            .find(|entry| Some(entry.id) == self.active)
            .map(|entry| &entry.terminal)
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        if let Some(terminal) = self.active_terminal() {
            terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
        } else {
            self.empty_focus.focus(window, cx);
        }
    }

    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.tabs_focus.contains_focused(window, cx)
            || self.empty_focus.contains_focused(window, cx)
            || self
                .entries
                .iter()
                .any(|entry| entry.terminal.read(cx).contains_focus(window, cx))
    }

    #[cfg(all(test, feature = "gui-tests"))]
    pub fn has_sessions(&self) -> bool {
        !self.entries.is_empty()
    }

    pub fn has_running_sessions(&self, cx: &App) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.terminal.read(cx).is_running(cx))
    }

    pub fn shutdown(&mut self, cx: &mut Context<Self>) -> Shutdown {
        if let Some(shutdown) = &self.shutdown {
            return Box::pin(shutdown.clone());
        }
        self.closing = true;
        self.active = None;
        let mut pending = std::mem::take(&mut self.retiring);
        for entry in self.entries.drain(..) {
            pending.push(
                entry
                    .terminal
                    .update(cx, |terminal, cx| terminal.shutdown(cx))
                    .shared(),
            );
        }
        let shutdown: Shutdown = Box::pin(async move {
            for pending in pending {
                pending.await;
            }
        });
        let shutdown = shutdown.shared();
        cx.background_executor().spawn(shutdown.clone()).detach();
        self.shutdown = Some(shutdown.clone());
        cx.notify();
        Box::pin(shutdown)
    }

    pub(super) fn new_terminal(
        &mut self,
        _: &NewTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(cwd) = self.cwd.clone() {
            self.open(cwd, window, cx);
        }
    }

    fn close_terminal(&mut self, _: &CloseTerminal, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.active {
            self.request_close(id, window, cx);
        }
    }

    fn focus_terminal(&mut self, _: &FocusTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.focus(window, cx);
    }

    fn next_terminal(&mut self, _: &NextTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle(false, window, cx);
    }

    fn previous_terminal(
        &mut self,
        _: &PreviousTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle(true, window, cx);
    }

    fn cycle(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.entries.is_empty() || self.closing {
            return;
        }
        let index = self
            .entries
            .iter()
            .position(|entry| Some(entry.id) == self.active)
            .unwrap_or(0);
        let count = self.entries.len();
        self.active = Some(self.entries[(index + if reverse { count - 1 } else { 1 }) % count].id);
        self.tabs_focus.focus(window, cx);
        cx.notify();
    }

    fn remove(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return;
        };
        let entry = self.entries.remove(index);
        let pending = entry
            .terminal
            .update(cx, |terminal, cx| terminal.shutdown(cx))
            .shared();
        cx.background_executor().spawn(pending.clone()).detach();
        self.retiring.retain(|pending| pending.peek().is_none());
        self.retiring.push(pending);
        if self.active == Some(id) {
            self.active = self
                .entries
                .get(index.min(self.entries.len().saturating_sub(1)))
                .map(|entry| entry.id);
        }
        self.focus(window, cx);
        cx.notify();
    }

    pub(super) fn request_close(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.closing {
            self.remove(id, window, cx);
        }
    }

    pub fn choices(&self, cx: &App) -> Vec<(u64, SharedString, bool)> {
        self.entries
            .iter()
            .map(|entry| {
                (
                    entry.id,
                    Self::title(entry, cx),
                    self.active == Some(entry.id),
                )
            })
            .collect()
    }

    pub fn active_id(&self) -> Option<u64> {
        self.active
    }

    pub(super) fn select(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.closing && self.entries.iter().any(|entry| entry.id == id) {
            self.active = Some(id);
            self.focus(window, cx);
            cx.notify();
        }
    }

    pub fn active_title(&self, cx: &App) -> SharedString {
        self.entries
            .iter()
            .find(|entry| Some(entry.id) == self.active)
            .map(|entry| Self::title(entry, cx))
            .unwrap_or_else(|| tr!("local-terminal-title").into())
    }

    fn title(entry: &Entry, cx: &App) -> SharedString {
        if let Some(title) = entry
            .terminal
            .read(cx)
            .title(cx)
            .filter(|title| !title.trim().is_empty())
        {
            return title;
        }
        entry
            .terminal
            .read(cx)
            .cwd()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| tr!("local-terminal-title").to_owned())
            .into()
    }

    pub fn active_hint(&self, cx: &App) -> Option<gds::ui::tooltip::Details> {
        self.entries
            .iter()
            .find(|entry| Some(entry.id) == self.active)
            .map(|entry| Self::hint(entry, cx))
    }

    fn hint(entry: &Entry, cx: &App) -> gds::ui::tooltip::Details {
        let cwd = entry.terminal.read(cx).cwd().display().to_string();
        let status = match entry.terminal.read(cx).status(cx) {
            Status::Starting => tr!("local-terminal-starting").to_owned(),
            Status::Running => tr!("local-terminal-running").to_owned(),
            Status::Stopping => tr!("local-terminal-closing").to_owned(),
            Status::Closed => tr!("local-terminal-closed").to_owned(),
            Status::Exited { code, .. } => {
                tr!("local-terminal-exited", code = code.to_string())
            }
            Status::Failed(error) => error,
        };
        let origin = tr!("local-terminal-started-in", path = cwd);
        gds::ui::tooltip::Details::new(Self::title(entry, cx))
            .line(origin)
            .line(status)
    }

    fn tab(&self, entry: &Entry, window: &Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let id = entry.id;
        let title = Self::title(entry, cx);
        let hint = Self::hint(entry, cx);
        let selected = self.active == Some(id);
        let shortcut_target = selected && self.tabs_focus.is_focused(window);
        let group = SharedString::from(format!("agents-terminal-row-{id}"));
        let close_label = tr!("local-terminal-tab-close");
        let close = Button::icon_button(("agents-terminal-close", id))
            .control_icon(IconName::X)
            .accessibility_label(close_label)
            .selected(selected)
            .when(shortcut_target, |button| {
                button.tooltip_with_shortcut(
                    close_label,
                    gds::ui::Shortcut::new(Box::new(CloseTerminal), Some(TABS_CONTEXT)),
                )
            })
            .when(!shortcut_target, |button| button.tooltip(close_label))
            .on_click(cx.listener(move |panel, _, window, cx| {
                cx.stop_propagation();
                panel.request_close(id, window, cx);
            }))
            .into_any_element();
        let reveal_close = shortcut_target
            && gds::ui::HeldModifier::from_modifiers(window.modifiers())
                == Some(gds::ui::HeldModifier::Ctrl);
        let close = if reveal_close {
            close
        } else {
            gds::ui::hover_action(group.clone(), div().into_any_element(), Some(close))
        };
        div()
            .id(("agents-terminal-row", id))
            .test_support()
            .group(group)
            .w_full()
            .flex_shrink_0()
            .tooltip_show_delay(gds::appearance::tokens::TOOLTIP_DELAY)
            .hoverable_tooltip(move |window, cx| hint.view(window, cx))
            .child(
                gds::ui::NavItem::new(("agents-terminal-tab", id), IconName::Terminal, title)
                    .selected(selected)
                    .tooltip_enabled(false)
                    .trailing(vec![close])
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.active = Some(id);
                        panel.tabs_focus.focus(window, cx);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Navigation is rendered with the window's current modifier and focus scope.
    pub fn render_tabs(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        Tabs(cx.entity()).into_any_element()
    }

    fn tabs(&self, window: &Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        div()
            .id("agents-terminal-tabs")
            .test_support()
            .key_context(TABS_CONTEXT)
            .track_focus(&self.tabs_focus)
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .text_color(gds::appearance::theme::Colors::current(cx).sidebar_foreground)
            .on_modifiers_changed(cx.listener(|_, _, _, cx| cx.notify()))
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::close_terminal))
            .on_action(cx.listener(Self::next_terminal))
            .on_action(cx.listener(Self::previous_terminal))
            .on_action(cx.listener(Self::focus_terminal))
            .children(self.entries.iter().map(|entry| self.tab(entry, window, cx)))
            .child(
                gds::ui::NavItem::new(
                    "agents-terminal-new",
                    IconName::Plus,
                    tr!("local-terminal-new"),
                )
                .disabled(self.cwd.is_none() || self.closing)
                .shortcut(gds::ui::Shortcut::new(
                    Box::new(NewTerminal),
                    Some(TABS_CONTEXT),
                ))
                .on_click(
                    cx.listener(|panel, _, window, cx| {
                        panel.new_terminal(&NewTerminal, window, cx)
                    }),
                ),
            )
            .into_any_element()
    }
}

#[derive(IntoElement)]
struct Tabs(Entity<Panel>);

impl RenderOnce for Tabs {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.0.update(cx, |panel, cx| panel.tabs(window, cx))
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.active_terminal() {
            Some(terminal) => terminal.clone().into_any_element(),
            None => div()
                .size_full()
                .key_context(TABS_CONTEXT)
                .track_focus(&self.empty_focus)
                .on_action(cx.listener(Self::new_terminal))
                .on_action(cx.listener(Self::focus_terminal))
                .p_3()
                .bg(gds::appearance::theme::content_background(cx))
                .text_color(cx.theme().muted_foreground)
                .child(if self.cwd.is_some() {
                    tr!("local-terminal-empty")
                } else {
                    tr!("local-terminal-no-cwd")
                })
                .into_any_element(),
        };
        div()
            .size_full()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(content)
    }
}
