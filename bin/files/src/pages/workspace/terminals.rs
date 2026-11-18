//! Terminal sessions belong to the window; the dock is only their current surface.
use super::{
    model::{Pane, Prompt},
    ui::Browser,
};
use crate::shared::ui::actions::Command;
use anti_webui::prelude::*;
use gds::{
    appearance::{motion, tokens},
    i18n,
    ui::{
        Frame, Icon, IconButton, NavItem,
        sidebar::SidebarPresentation,
        split,
        terminal::{Status, Terminal},
    },
};
use gds_uim::el;
use std::{cell::Cell, rc::Rc};
#[derive(Clone)]
struct Session {
    id: u64,
    title: Signal<Option<String>>,
    terminal: Terminal,
}
const MIN_CONTENT_HEIGHT: f32 =
    94.0 + 108.0 + split::DIVIDER_HEIGHT + gds::appearance::tokens::SIDEBAR_ITEM_HEIGHT * 2.0;
const MIN_DOCK_HEIGHT: f32 = 160.0;
const DEFAULT_DOCK_HEIGHT: f32 = 280.0;

#[derive(Clone)]
pub(super) struct Dock {
    sessions: Signal<Vec<Session>>,
    active: Signal<Option<u64>>,
    next: Rc<Cell<u64>>,
    height: Signal<f32>,
    maximized: Signal<bool>,
    available: Signal<f32>,
    drag_height: Rc<Cell<f32>>,
    tabs_node: NodeRef,
    tabs_focused: Signal<bool>,
}
impl Dock {
    pub fn new() -> Self {
        Self {
            sessions: signal(Vec::new()),
            active: signal(None),
            next: Rc::new(Cell::new(1)),
            height: signal(DEFAULT_DOCK_HEIGHT),
            maximized: signal(false),
            available: signal(600.0),
            drag_height: Rc::new(Cell::new(DEFAULT_DOCK_HEIGHT)),
            tabs_node: node_ref(),
            tabs_focused: signal(false),
        }
    }
    fn active(&self) -> Option<Session> {
        let id = self.active.get_untracked();
        self.sessions.with_untracked(|sessions| {
            sessions
                .iter()
                .find(|session| Some(session.id) == id)
                .cloned()
        })
    }
    fn constrain_height(&self, height: f32) -> f32 {
        let available = self.available.get_untracked();
        let content_minimum = MIN_CONTENT_HEIGHT.min(available / 2.0);
        let maximum = (available - content_minimum).max(0.0);
        height.clamp(
            (MIN_DOCK_HEIGHT + split::DIVIDER_HEIGHT).min(maximum),
            maximum,
        )
    }
}
impl Browser {
    pub(super) fn apply_terminal_preferences(
        &self,
        preferences: &crate::entities::preferences::Preferences,
    ) {
        if !preferences.terminal_enabled() {
            self.0.terminal.set(false);
            self.0.terminals.maximized.set(false);
        }
        self.0.terminals.sessions.with_untracked(|sessions| {
            for session in sessions {
                session
                    .terminal
                    .set_preferences(preferences.terminal.clone());
            }
        });
    }
    pub(super) fn toggle_terminal(&self) {
        if !self.0.preferences.snapshot().terminal_enabled()
            || self.read(|state| state.picker.is_some())
        {
            return;
        }
        self.finish_address_edit();
        if self.pane() != Pane::Files {
            self.show_files();
        }
        let visible = !self.0.terminal.get_untracked();
        self.0.terminal.set(visible);
        if visible {
            if self.0.terminals.next.get() == 1 {
                self.new_terminal();
            } else if let Some(session) = self.0.terminals.active() {
                session.terminal.focus();
            } else {
                let tabs = self.0.terminals.tabs_node;
                request_frame(move |_| tabs.focus());
            }
        } else {
            self.focus_content();
        }
    }
    pub(super) fn new_terminal(&self) {
        let path = self.read(|state| {
            state
                .tab()
                .navigation
                .directory()
                .filter(|_| !state.tab().navigation.is_loading())
                .and_then(|_| state.tab().navigation.path())
                .filter(|path| path.is_absolute())
                .unwrap_or(&state.home)
                .to_owned()
        });
        self.new_terminal_at(path);
    }
    pub(super) fn new_terminal_at(&self, path: std::path::PathBuf) {
        if !self.0.preferences.snapshot().terminal_enabled()
            || self.read(|state| state.picker.is_some())
        {
            return;
        }
        let dock = self.0.terminals.clone();
        let preferences = self.0.preferences.snapshot().terminal;
        let terminal = self
            .0
            .owner
            .with(|| Terminal::new(path, preferences, self.scope()));
        let focus = terminal.clone();
        let title = self.0.owner.with(|| signal(None));
        let id = dock.next.get();
        dock.next.set(id + 1);
        dock.sessions.update(|sessions| {
            sessions.push(Session {
                id,
                title,
                terminal,
            })
        });
        dock.active.set(Some(id));
        self.0.terminal.set(true);
        focus.focus();
    }
    pub(super) fn observe_terminals(&self) {
        let browser = self.downgrade();
        effect(move || {
            let Some(browser) = browser.upgrade() else {
                return;
            };
            let closed = browser.0.terminals.sessions.with(|sessions| {
                sessions
                    .iter()
                    .filter(|session| {
                        matches!(
                            session.terminal.status(),
                            Status::Exited { signal: None, .. } | Status::Closed
                        )
                    })
                    .map(|session| session.id)
                    .collect::<Vec<_>>()
            });
            untrack(|| {
                for id in closed {
                    browser.close_terminal(id);
                }
            });
        });
    }
    fn close_terminal(&self, id: u64) {
        let dock = &self.0.terminals;
        let Some(index) = dock
            .sessions
            .with_untracked(|sessions| sessions.iter().position(|session| session.id == id))
        else {
            return;
        };
        let mut sessions = dock.sessions.get_untracked();
        let session = sessions.remove(index);
        let input_focused = session.terminal.is_focused();
        let tabs_focused = dock.tabs_focused.get_untracked();
        let shutdown = session.terminal.shutdown();
        self.work(
            "Close terminal session",
            async move {
                shutdown.await;
                Ok(())
            },
            |_, _| {},
        );
        if dock.active.get_untracked() == Some(id) {
            let next = sessions
                .get(index.min(sessions.len().saturating_sub(1)))
                .cloned();
            let tabs = dock.tabs_node;
            batch(|| {
                dock.sessions.set(sessions);
                dock.active.set(next.as_ref().map(|session| session.id));
            });
            if self.0.terminal.get_untracked()
                && self.pane() == Pane::Files
                && (input_focused || tabs_focused)
            {
                request_frame(move |_| {
                    if let Some(next) = next.filter(|_| input_focused) {
                        next.terminal.focus();
                    } else {
                        tabs.focus();
                    }
                });
            }
        } else {
            dock.sessions.set(sessions);
        }
    }
    fn rename_terminal(&self, id: u64) {
        let Some(session) =
            self.0.terminals.sessions.with_untracked(|sessions| {
                sessions.iter().find(|session| session.id == id).cloned()
            })
        else {
            return;
        };
        let name = session.label();
        self.prompt_name(i18n::text("files-terminal-rename"), name, move |title| {
            session
                .title
                .set((!title.trim().is_empty()).then_some(title.trim().to_owned()));
        });
    }
    fn terminal_key(&self, event: &KeyboardEvent) -> bool {
        if !matches!(self.0.prompt.get_untracked(), Prompt::Closed) {
            return false;
        }
        let plain = !event.modifiers.ctrl
            && !event.modifiers.alt
            && !event.modifiers.shift
            && !event.modifiers.logo;
        if plain && event.key == Key::Named(NamedKey::F(6)) {
            self.0.terminals.maximized.set(false);
            self.focus_content();
            return true;
        }
        if plain && event.key == Key::Named(NamedKey::F(2)) {
            if let Some(session) = self.0.terminals.active() {
                self.rename_terminal(session.id);
            }
            return true;
        }
        if let Some(command) = Command::from_key(event) {
            match command {
                Command::NewTab => self.new_terminal(),
                Command::CloseTab => {
                    if let Some(session) = self.0.terminals.active() {
                        self.close_terminal(session.id);
                    }
                }
                Command::Terminal
                | Command::Settings
                | Command::Help
                | Command::Menu
                | Command::FocusNotifications
                | Command::CloseNotification
                | Command::CloseAllNotifications => self.dispatch(command),
                _ => return false,
            }
            return true;
        }
        false
    }
    fn terminal_tabs_key(&self, event: &KeyboardEvent) {
        if event.modifiers != Modifiers::default() {
            return;
        }
        match event.key {
            Key::Named(NamedKey::ArrowDown) => self.cycle_terminal(false),
            Key::Named(NamedKey::ArrowUp) => self.cycle_terminal(true),
            Key::Named(NamedKey::Enter) => {
                if let Some(session) = self.0.terminals.active() {
                    session.terminal.focus();
                }
            }
            _ => return,
        }
        event.prevent_default();
        event.stop_propagation();
    }
    fn cycle_terminal(&self, backwards: bool) {
        let dock = &self.0.terminals;
        let active = dock.active.get_untracked();
        let next = dock.sessions.with_untracked(|sessions| {
            let current = sessions
                .iter()
                .position(|session| Some(session.id) == active)?;
            let offset = if backwards { sessions.len() - 1 } else { 1 };
            Some(sessions[(current + offset) % sessions.len()].id)
        });
        dock.active.set(next);
        dock.tabs_node.focus();
    }
    fn change_terminal_directory(&self) {
        let available = self.read(|state| state.tab().navigation.directory().is_some());
        if available
            && self
                .0
                .terminals
                .active()
                .is_some_and(|session| session.terminal.is_running())
        {
            // The existing shell's editing buffer is not observable; preserve it and open here.
            self.new_terminal();
        }
    }
    fn terminal_directory_control(&self) -> IconButton {
        let available = self.read(|state| state.tab().navigation.directory().is_some());
        self.0.terminals.active.get();
        let running = self
            .0
            .terminals
            .active()
            .is_some_and(|session| session.terminal.is_running());
        let browser = self.clone();
        el! {
            "change-terminal-directory".IconButton::new.{
            icon:Icon::FolderInput;
                disabled:$(!available || !running);
                label_fn:()=>"files-terminal-switch-directory".i18n::text;
                on_click:move _=>&browser.{change_terminal_directory};
            }
        }
    }
    pub(super) fn terminal_workspace_resized(&self, _: f32, available: f32) {
        let dock = &self.0.terminals;
        dock.available.set_if_changed(available);
        let preferred = self.0.preferences.snapshot().files.terminal_content_height;
        let height = preferred.map_or_else(|| DEFAULT_DOCK_HEIGHT, |content| available - content);
        dock.height.set_if_changed(dock.constrain_height(height));
    }
    fn resize_terminal(&self, event: &GrabEvent) {
        let dock = &self.0.terminals;
        match event.phase {
            GrabPhase::Start => dock.drag_height.set(dock.height.get_untracked()),
            GrabPhase::Move => dock
                .height
                .set(dock.constrain_height(dock.drag_height.get() - event.total.y)),
            GrabPhase::End => self.commit_terminal_size(),
            GrabPhase::Cancel => dock.height.set(dock.drag_height.get()),
        }
    }
    fn commit_terminal_size(&self) {
        let dock = &self.0.terminals;
        let content = (dock.available.get_untracked() - dock.height.get_untracked()).max(0.0);
        self.edit_preferences(|preferences| {
            preferences.files.terminal_content_height = Some(content)
        });
    }
    fn nudge_terminal(&self, step: f32) {
        let dock = &self.0.terminals;
        dock.height
            .set(dock.constrain_height(dock.height.get_untracked() - step));
        self.commit_terminal_size();
    }
    fn terminal_tab(&self, session: Session, presentation: SidebarPresentation) -> NavItem {
        let id = session.id;
        let active = self.0.terminals.active;
        let close = self.clone();
        let corner = self.clone();
        let middle = self.clone();
        let tabs = self.0.terminals.tabs_node;
        el! { (&session).{label}.NavItem::new.{
            icon:Icon::Terminal;
            label_fn:move()=>&session.{label};
            active_when:move()=>$(active.get()==Some(id));
            presentation:presentation;
            on_click:move _=>${
                active.set(Some(id));
                tabs.focus();
            };
            on_aux_click: move event => ${
                if event.button == Some(PointerButton::Middle) {
                    event.stop_propagation();
                    middle.close_terminal(id);
                }
            };
            trailing:"close-terminal".IconButton::new.{icon:Icon::Close;label_fn:()=>"files-terminal-end".i18n::text;on_click:move event=>${event.stop_propagation();close.close_terminal(id);}};
            corner_action:"close-terminal-compact".IconButton::new.{
                icon:Icon::Close; compact; label_fn:()=>"files-terminal-end".i18n::text;
                on_click:move event=>${event.stop_propagation();corner.close_terminal(id);};
            };
        } }
    }
    fn terminal_tabs(&self, presentation: SidebarPresentation) -> Div {
        let sessions = self.0.terminals.sessions;
        let focused = self.0.terminals.tabs_focused;
        let browser = self.clone();
        let keys = self.clone();
        el! { .div.{
            flex;flex_col;size_full;min_h:0.0_f32;overflow_y_scroll;p:tokens::SIDEBAR_PADDING;gap:4.0_f32;
            node_ref:$(self.0.terminals.tabs_node);focusable;
            on_focus_change: move value => focused.{set: value};
            on_key_down:move event=>&keys.{terminal_tabs_key:event};
        }.[(move()=>sessions.{get},session=>session->id,move session=>&browser.{terminal_tab:session,presentation}).for_each] }
    }
    pub(super) fn terminal_view(&self, presentation: SidebarPresentation) -> Div {
        let browser = self.clone();
        let shown = follow(
            move || {
                if browser.0.terminal.get() && browser.pane() == Pane::Files {
                    1.0_f32
                } else {
                    0.0_f32
                }
            },
            motion::WORKSPACE,
        );
        let dock = self.0.terminals.clone();
        let maximized = dock.maximized;
        let available = dock.available;
        let height = dock.height;
        let expanded = follow(
            move || if maximized.get() { 1.0_f32 } else { 0.0_f32 },
            motion::WORKSPACE,
        );
        let browser = self.clone();
        el! { .div.{
            flex; flex_col; flex_none; min_h: 0.0_f32; overflow_hidden;
            dyn_style: move s => ${
                let shown = shown.get();
                let expanded = expanded.get();
                let height = (height.get() + (available.get() - height.get()) * expanded) * shown;
                if shown == 0.0 { s.hidden() } else { s.h(height) }
            };
        }.[
            (move () => $(shown.get() > 0.0), move () => $(browser.terminal_dock(presentation, shown, expanded))).show,
        ] }
    }
    fn terminal_dock(
        &self,
        presentation: SidebarPresentation,
        shown: Animated<f32>,
        expanded: Animated<f32>,
    ) -> Div {
        let dock = self.0.terminals.clone();
        let maximize = dock.maximized;
        let active = dock.active;

        let add = self.clone();
        let resize = self.clone();
        let nudge = self.clone();
        let keys = self.clone();
        let body_keys = self.clone();
        let change_directory = self.clone();
        let sessions = dock.sessions;
        let frame = Frame::default().sidebar_width_binding(presentation.width_binding());
        let heading = el! { .div.{flex;items_center;h_full;px:tokens::SIDEBAR_PADDING;min_w:0.0_f32}.[
            .div.{flex_1;min_w:0.0_f32;overflow_hidden;px:tokens::SIDEBAR_SECTION_PADDING;
                dyn_style:move s=>$(s.opacity(presentation.progress()).text_color(theme().text_muted));
            }.[(()=>"settings-terminal".i18n::text).text.{text_size:12.0_f32}],
            "new-terminal".IconButton::new.{icon:Icon::Add;label_fn:()=>"files-terminal-new".i18n::text;on_click:move _=>&add.{new_terminal}},
        ] };
        let title = dock.clone();
        let hint = dock.clone();
        let title = el! { .div.{flex_1; min_w:0.0_f32}.[
            (move() => ${
                let _ = title.active.get();
                title.active().map_or_else(|| i18n::text("settings-terminal"), |session| session.label())
            }).text.{ellipsis},
        ] };
        let title = tooltip_with(title, TooltipOptions::new(), move |_| {
            let hint = hint.clone();
            move || {
                let hint = hint.clone();
                el! { .div.{flex; flex_col; gap:4.0_f32; px:8.0_f32; py:4.0_f32; rounded:4.0_f32;
                    dyn_style: s => $(s.bg(theme().tooltip).text_color(theme().on_tooltip));
                }.[
                    (move () => ${
                        let _ = hint.active.get();
                        hint.active().map_or_else(|| i18n::text("settings-terminal"), |session| {
                            format!("{}\n{}\n{}", session.label(), session.terminal.cwd().display(), session.terminal.status_label())
                        })
                    }).text,
                ] }
            }
        });
        let controls = el! { .div.{flex;items_center;h_full;min_w:0.0_f32;gap:4.0_f32;px:tokens::TERMINAL_CONTENT_INSET}.[
            title,
            (move()=>&change_directory.{terminal_directory_control}).dyn_view,
            "maximize-terminal".IconButton::new.{
                icon_fn: move () => $(if maximize.get() { Icon::Contract } else { Icon::Expand });
                label_fn: move () => $(i18n::text(if maximize.get(){"files-terminal-restore"}else{"files-terminal-maximize"}));
                on_click: move _ => $(maximize.update(|value|*value = !*value));
            },
        ] };
        let header = frame
            .workspace(heading, controls)
            .flex_grow(0.0_f32)
            .flex_basis(Length::Auto)
            .flex_none()
            .h(tokens::TERMINAL_HEADER_HEIGHT);
        let handle = split::handle(
            "files-terminal-resize",
            move |event| resize.resize_terminal(event),
            move |step| nudge.nudge_terminal(step),
        );
        let divider = el! { $(frame.workspace(div(), div())).{
            flex_grow: 0.0_f32; flex_basis: Length::Auto; flex_none;
            dyn_style: move s => $(s.h(split::DIVIDER_HEIGHT * shown.get() * (1.0 - expanded.get())));
        }.[
            handle.{absolute; inset:0.0_f32; h: Length::Auto;
                dyn_style: move s => $(if shown.get() == 1.0 && expanded.get() == 0.0 {s} else {s.hidden()});
            },
        ] };
        let body = move || {
            let id = active.get();
            untrack(|| {
                match sessions.with_untracked(|sessions| {
                    sessions
                        .iter()
                        .find(|session| Some(session.id) == id)
                        .cloned()
                }) {
                    Some(session) => session.view(body_keys.clone()).into_view(),
                    None => el! { .div.{size_full; p:tokens::TERMINAL_CONTENT_INSET;
                        dyn_style: s => $(s.bg(gds::app::Gds::current().appearance().theme().content_background()).text_color(theme().text_muted));
                    }.[(()=>"files-terminal-empty".i18n::text).text] }.into_view(),
                }
            })
        };
        el! {
            .div.{
                flex; flex_col; min_h:0.0_f32; flex_1; overflow_hidden; border_t:1.0_f32;
                on_key_down:move event=>${
                    if keys.terminal_key(event) {
                        event.prevent_default();
                        event.stop_propagation();
                    }
                };
                dyn_style: s => $(s.border_color(theme().border));
            }.[
                divider,
                header,
                $(frame.without_workspace_content_background().workspace(self.terminal_tabs(presentation), dyn_view(body))),
            ]
        }
    }
}
impl Session {
    fn label(&self) -> String {
        self.title.get().unwrap_or_else(|| {
            self.terminal.title().unwrap_or_else(|| {
                self.terminal
                    .cwd()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
        })
    }
    fn view(self, browser: Browser) -> Div {
        let surface = self
            .terminal
            .view_with_key_handler(move |event| browser.terminal_key(event));
        let status = self.terminal.clone();
        let description = move || {
            let message = if status.status() == Status::Running {
                String::new()
            } else {
                status.status_label()
            };
            let error = status.error().unwrap_or_default();
            el! { .div.{
                px:tokens::TERMINAL_CONTENT_INSET; text_size:12.0_f32;
                dyn_style: s => $(s.bg(gds::app::Gds::current().appearance().theme().content_background()));
            }.[
                if $(!message.is_empty()) [message.text],
                if $(!error.is_empty()) [error.text],
            ] }
        };
        el! {
            .div.{flex;flex_col;flex_1;min_h:0.0_f32}.[
                surface,
                description.dyn_view,
            ]
        }
    }
}
