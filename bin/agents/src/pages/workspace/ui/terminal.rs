//! Local shells belong to a backend/work-directory pair, never to a conversation.

use super::*;
use crate::shared::i18n::tr;
use gpui_kit::component::menu::PopupMenuItem;
use std::path::Path;

mod panel;
use panel::Panel;

pub(crate) use crate::shared::config::TERMINAL_TABS_CONTEXT as TABS_CONTEXT;

actions!(
    agents_terminal,
    [
        NewTerminal,
        CloseTerminal,
        NextTerminal,
        PreviousTerminal,
        FocusTerminal,
        FocusTerminalComposer
    ]
);

pub(super) fn bind_keys(cx: &mut App) {
    use gds::ui::terminal::{
        Copy, INPUT_CONTEXT, Paste, ScrollBottom, ScrollPageDown, ScrollPageUp, ScrollTop,
    };
    cx.bind_keys([
        KeyBinding::new("ctrl-shift-c", Copy, Some(INPUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-v", Paste, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-pageup", ScrollPageUp, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-pagedown", ScrollPageDown, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-home", ScrollTop, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-end", ScrollBottom, Some(INPUT_CONTEXT)),
        KeyBinding::new("f6", FocusTerminalComposer, Some(INPUT_CONTEXT)),
        KeyBinding::new("f6", FocusTerminalComposer, Some(TABS_CONTEXT)),
        KeyBinding::new("ctrl-n", NewTerminal, Some(TABS_CONTEXT)),
        KeyBinding::new("ctrl-w", CloseTerminal, Some(TABS_CONTEXT)),
        KeyBinding::new("down", NextTerminal, Some(TABS_CONTEXT)),
        KeyBinding::new("up", PreviousTerminal, Some(TABS_CONTEXT)),
        KeyBinding::new("enter", FocusTerminal, Some(TABS_CONTEXT)),
    ]);
}

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Group {
    server: Target,
    cwd: PathBuf,
}

impl Group {
    fn new(server: &Target, cwd: Option<&Path>) -> Option<Self> {
        if !local_ipc(&server.target) {
            return None;
        }
        let cwd = cwd.filter(|path| path.is_absolute())?;
        Some(Self {
            server: server.clone(),
            cwd: navigation::groups::normalize(cwd),
        })
    }
}

fn local_ipc(target: &bas::service::Target) -> bool {
    use bas::service::Target;
    match target {
        Target::Local => target.resolved().is_ok_and(|target| match target {
            Target::Unix(_) => cfg!(unix),
            Target::Pipe(name) => cfg!(windows) && local_pipe(&name),
            _ => false,
        }),
        Target::Unix(_) => cfg!(unix),
        Target::Pipe(name) => cfg!(windows) && local_pipe(name),
        // Loopback TCP can be forwarded to another machine. Never infer locality.
        Target::Tcp(_) | Target::Mtls(_) => false,
    }
}

fn local_pipe(name: &str) -> bool {
    name.starts_with(r"\\.\pipe\") || (!name.contains('\\') && !name.contains('/'))
}

/// Resources provided by one directory workspace, retained while another is selected.
struct WorkspaceTerminal {
    panel: Entity<Panel>,
    split: Entity<gds::ui::split::State>,
    _changes: Subscription,
    _split_changes: Subscription,
}

pub(super) struct Terminals {
    workspaces: HashMap<Group, WorkspaceTerminal>,
    pub visible: bool,
    pub(super) unavailable_split: Entity<gds::ui::split::State>,
}

impl Terminals {
    pub fn new(cx: &mut App) -> Self {
        Self {
            workspaces: HashMap::new(),
            visible: false,
            unavailable_split: cx.new(|cx| gds::ui::split::State::new(None, 160., 140., cx)),
        }
    }
}

impl Workspace {
    fn terminal_group(&self) -> Option<Group> {
        let cwd = if self.selected.id.is_some() {
            // A session without cwd must not inherit the previously selected directory.
            self.current().and_then(|session| session.cwd.as_deref())
        } else {
            self.navigation.selected_directory.as_deref()
        };
        Group::new(&self.backend.server, cwd)
    }

    pub(super) fn terminal_panel(&self) -> Option<Entity<Panel>> {
        self.terminals
            .workspaces
            .get(&self.terminal_group()?)
            .map(|workspace| workspace.panel.clone())
    }

    pub(super) fn sync_terminal_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.terminals.visible || self.window.compact {
            return;
        }
        let Some(group) = self.terminal_group() else {
            return;
        };
        if !self.terminals.workspaces.contains_key(&group) {
            let preferences = crate::shared::config::terminal::current(cx);
            let panel = cx.new(|cx| {
                let mut panel = Panel::new(cx);
                panel.set_preferences(preferences, cx);
                panel.set_cwd(Some(group.cwd.clone()), cx);
                panel
            });
            let changes = cx.observe(&panel, |_, _, cx| cx.notify());
            let split = cx.new(|cx| {
                let mut split = gds::ui::split::State::new(None, 160., 140., cx);
                split.default_bottom(260.);
                split
            });
            let split_changes = cx.observe(&split, |_, _, cx| cx.notify());
            self.terminals.workspaces.insert(
                group,
                WorkspaceTerminal {
                    panel,
                    split,
                    _changes: changes,
                    _split_changes: split_changes,
                },
            );
        }
        if let Some(panel) = self.terminal_panel()
            && panel.read(cx).needs_initial_reveal()
        {
            // This sync also runs during rendering. Start only after the frame,
            // provided this owner is still visible and selected.
            let owner = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.terminals.visible
                        && this.window.presented
                        && !this.window.compact
                        && this.terminal_panel().is_some_and(|active| active == panel)
                        && panel.read(cx).needs_initial_reveal()
                    {
                        panel.update(cx, |panel, cx| panel.reveal(window, cx));
                    }
                });
            });
        }
    }

    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.window.compact {
            self.toggle_mode(window, cx);
            self.terminals.visible = true;
        } else {
            self.terminals.visible = !self.terminals.visible;
        }
        self.sync_terminal_panel(window, cx);
        if self.terminals.visible {
            if let Some(panel) = self.terminal_panel() {
                panel.update(cx, |panel, cx| panel.reveal(window, cx));
            }
        } else {
            self.focus_input(window, cx);
        }
        cx.notify();
    }

    pub(super) fn on_terminal_focus_composer(
        &mut self,
        _: &FocusTerminalComposer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            self.focus_input(window, cx);
            cx.stop_propagation();
        } else {
            cx.propagate();
        }
    }

    pub(super) fn terminal_focused(&self, window: &Window, cx: &App) -> bool {
        self.terminals
            .workspaces
            .values()
            .any(|workspace| workspace.panel.read(cx).contains_focus(window, cx))
    }

    pub(crate) fn has_running_terminals(&self, cx: &App) -> bool {
        self.terminals
            .workspaces
            .values()
            .any(|workspace| workspace.panel.read(cx).has_running_sessions(cx))
    }

    pub(crate) fn shutdown_terminals(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Vec<gds::ui::terminal::Shutdown> {
        self.terminals
            .workspaces
            .values()
            .map(|workspace| workspace.panel.update(cx, |panel, cx| panel.shutdown(cx)))
            .collect()
    }

    pub(super) fn update_terminal_preferences(&mut self, cx: &mut Context<Self>) {
        let preferences = crate::shared::config::terminal::current(cx);
        for workspace in self.terminals.workspaces.values() {
            workspace.panel.update(cx, |panel, cx| {
                panel.set_preferences(preferences.clone(), cx)
            });
        }
        cx.notify();
    }

    pub(super) fn terminal_toggle_button(&self, cx: &mut Context<Self>) -> Button {
        let visible = self.terminals.visible && !self.window.compact;
        let label = if visible {
            tr!("local-terminal-hide")
        } else {
            tr!("local-terminal-show")
        };
        layout::icon_button("toggle-local-terminal", IconName::Terminal)
            .selected(visible)
            .toggled(visible)
            .accessibility_label(label)
            .tooltip_with_shortcut(
                label,
                gds::ui::Shortcut::new(Box::new(ToggleTerminal), Some("AgentsHost")),
            )
            .on_click(cx.listener(|this, _, window, cx| this.toggle_terminal(window, cx)))
    }

    /// Only the matching workspace contributes its terminal controls to navigation.
    pub(super) fn render_workspace_terminals(
        &self,
        directory: Option<&Path>,
        presentation: gds::ui::sidebar::SidebarPresentation,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.terminals.visible {
            return None;
        }
        let group = Group::new(&self.backend.server, directory)?;
        if self.terminal_group().as_ref() != Some(&group) {
            return None;
        }
        let panel = self.terminals.workspaces.get(&group)?.panel.clone();
        let tabs = panel.update(cx, |panel, cx| panel.render_tabs(presentation, cx));
        Some(
            div()
                .id("workspace-terminals")
                .test_support()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .pl(px(presentation.interpolate(8., 0.)))
                .child(
                    gds::ui::sidebar::SideBarSectionTitle::new(gds::i18n::t!(
                        @cx, "local-terminal-title"
                    ))
                    .id("agents-terminal-heading")
                    .px(px(gds::appearance::tokens::SIDEBAR_SECTION_PADDING))
                    .py_1()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .opacity(presentation.label_opacity()),
                )
                .child(tabs)
                .into_any_element(),
        )
    }

    fn terminal_picker(
        &self,
        panel: Option<&Entity<Panel>>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let choices = panel
            .map(|panel| panel.read(cx).choices(cx))
            .unwrap_or_default();
        if choices.len() < 2 {
            return None;
        }
        let owner = cx.entity().downgrade();
        let panel = panel.cloned();
        let parent = self.window.handle;
        Some(
            self.menu_button(
                layout::icon_button("choose-local-terminal", IconName::List)
                    .accessibility_label(tr!("local-terminal-select"))
                    .tooltip(tr!("local-terminal-select")),
                choices.len(),
                false,
                move |mut menu, _, _| {
                    for (id, title, selected) in &choices {
                        let id = *id;
                        let owner = owner.clone();
                        let panel = panel.clone();
                        menu = menu.item(
                            PopupMenuItem::new(title.clone())
                                .icon(IconName::Terminal)
                                .checked(*selected)
                                .on_click(move |_, window, cx| {
                                    menus::in_parent(parent, window, cx, |window, cx| {
                                        let _ = owner.update(cx, |this, cx| {
                                            if let Some(panel) = &panel
                                                && this.terminal_panel().as_ref() == Some(panel)
                                            {
                                                panel.update(cx, |panel, cx| {
                                                    panel.select(id, window, cx)
                                                });
                                            }
                                        });
                                    });
                                }),
                        );
                    }
                    menu
                },
                cx,
            ),
        )
    }

    pub(super) fn render_terminal_dock(
        &self,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.terminals.visible {
            return content;
        }
        let workspace = self
            .terminal_group()
            .and_then(|group| self.terminals.workspaces.get(&group));
        let split = workspace
            .map(|workspace| workspace.split.clone())
            .unwrap_or_else(|| self.terminals.unavailable_split.clone());
        let panel = workspace.map(|workspace| workspace.panel.clone());
        let close_target = panel
            .as_ref()
            .and_then(|panel| panel.read(cx).active_id().map(|id| (panel.clone(), id)));
        let picker = self.terminal_picker(panel.as_ref(), cx);
        let active_title = panel
            .as_ref()
            .map(|panel| panel.read(cx).active_title(cx))
            .unwrap_or_else(|| tr!("local-terminal-title").into());
        let title_hint = panel
            .as_ref()
            .and_then(|panel| panel.read(cx).active_hint(cx))
            .unwrap_or_else(|| gds::ui::tooltip::Details::new(active_title.clone()));
        let controls = div()
            .flex()
            .items_center()
            .min_w_0()
            .px(px(gds::appearance::tokens::TERMINAL_CONTENT_INSET))
            .h(px(gds::appearance::tokens::TERMINAL_HEADER_HEIGHT))
            .gap_1()
            .child(
                div()
                    .id("active-local-terminal-title")
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(gds::appearance::tokens::SMALL)
                    .hoverable_tooltip(move |window, cx| title_hint.view(window, cx))
                    .child(active_title),
            )
            .children(picker)
            .child(
                layout::icon_button("close-local-terminal", IconName::X)
                    .accessibility_label(tr!("local-terminal-tab-close"))
                    .disabled(close_target.is_none())
                    .tooltip(tr!("local-terminal-tab-close"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some((panel, id)) = &close_target
                            && this.terminal_panel().as_ref() == Some(panel)
                        {
                            panel.update(cx, |panel, cx| panel.request_close(*id, window, cx));
                        }
                    })),
            )
            .child(
                layout::icon_button("new-local-terminal", IconName::Plus)
                    .accessibility_label(tr!("local-terminal-new"))
                    .disabled(workspace.is_none())
                    .tooltip(tr!("local-terminal-new"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(panel) = this.terminal_panel() {
                            panel.update(cx, |panel, cx| {
                                panel.new_terminal(&NewTerminal, window, cx)
                            });
                        }
                    })),
            );
        let header = controls
            .flex_none()
            .bg(gds::appearance::theme::content_background(cx));
        let body = workspace
            .map(|workspace| workspace.panel.clone().into_any_element())
            .unwrap_or_else(|| {
                div()
                    .size_full()
                    .p_3()
                    .bg(gds::appearance::theme::content_background(cx))
                    .text_color(cx.theme().muted_foreground)
                    .child(if local_ipc(&self.backend.server.target) {
                        tr!("local-terminal-no-cwd")
                    } else {
                        tr!("local-terminal-remote")
                    })
                    .into_any_element()
            });
        let dock = div()
            .id("local-terminal-dock")
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
                    .child(body),
            );
        let divider = div()
            .size_full()
            .bg(gds::appearance::theme::content_background(cx));
        let content = div()
            .size_full()
            .bg(gds::appearance::theme::content_background(cx))
            .child(content);
        gds::ui::split::VerticalSplit::new(split)
            .top(content)
            .bottom(dock)
            .label(tr!("local-terminal-resize"))
            .divider_background(divider)
            .into_any_element()
    }
}
