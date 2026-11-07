//! Thread shortcuts route through the same guarded operations as the visible controls.

use super::*;
use gpui_kit::{Action, AnyView, component::menu::PopupMenuItem};

pub(super) fn bind_keys(cx: &mut App) {
    for context in [
        "BasWorkspace && !GdsTerminal && !AgentsTerminalTabs",
        "BasWorkspace > Input && !GdsTerminal && !AgentsTerminalTabs",
        "BasWorkspace > BasComposer > Input && !GdsTerminal && !AgentsTerminalTabs",
    ] {
        cx.bind_keys([
            KeyBinding::new("f2", RenameThread, Some(context)),
            KeyBinding::new("ctrl-w", ArchiveThread, Some(context)),
            KeyBinding::new("ctrl-alt-p", ToggleThreadPin, Some(context)),
            KeyBinding::new("ctrl-d", ToggleThreadBookmark, Some(context)),
            KeyBinding::new("ctrl-alt-w", DeleteThread, Some(context)),
            KeyBinding::new("ctrl-alt-n", AddWorkspace, Some(context)),
            KeyBinding::new("ctrl-s", ToggleSession, Some(context)),
            KeyBinding::new("ctrl-m", OpenSessionManager, Some(context)),
            KeyBinding::new("ctrl-home", ScrollConversationToTop, Some(context)),
            KeyBinding::new("pageup", PageConversationUp, Some(context)),
            KeyBinding::new("pagedown", PageConversationDown, Some(context)),
            KeyBinding::new("ctrl-pageup", PreviousConversationTurn, Some(context)),
            KeyBinding::new("ctrl-pagedown", NextConversationTurn, Some(context)),
        ]);
    }
}

pub(super) fn menu_item(label: &'static str, action: Option<Box<dyn Action>>) -> PopupMenuItem {
    application::action_item(label, action, Some("BasWorkspace"))
}

pub(super) fn hint(label: &str, action: &dyn Action, window: &Window) -> String {
    gds::ui::Shortcut::new(action.boxed_clone(), Some("BasWorkspace")).hint(label, window)
}

pub(super) fn shortcut(action: impl Action) -> gds::ui::Shortcut {
    gds::ui::Shortcut::new(Box::new(action), Some("BasWorkspace"))
}

pub(super) fn tooltip(
    label: &'static str,
    action: impl Action,
) -> impl Fn(&mut Window, &mut App) -> AnyView {
    gds::ui::shortcut_tooltip(label, shortcut(action))
}

impl Workspace {
    pub(super) fn on_toggle_session(
        &mut self,
        _: &ToggleSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        let Some(meta) = self.current().filter(|meta| !meta.archived) else {
            return;
        };
        let id = meta.id;
        if meta.status == SessionStatus::Stopped {
            self.request_restart_agent_session(id, cx);
        } else {
            self.request_close_agent_session(id, cx);
        }
    }

    pub(super) fn on_open_session_manager(
        &mut self,
        _: &OpenSessionManager,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        let agent = self
            .current()
            .map(|meta| meta.agent.to_string())
            .unwrap_or_else(|| self.selected.agent.clone());
        self.open_agent_history(agent, window, cx);
    }

    pub(super) fn on_conversation_top(
        &mut self,
        _: &ScrollConversationToTop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.scroll_conversation_to_top(cx);
    }

    pub(super) fn on_conversation_page_up(
        &mut self,
        _: &PageConversationUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.page_conversation(true, cx);
    }

    pub(super) fn on_conversation_page_down(
        &mut self,
        _: &PageConversationDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.page_conversation(false, cx);
    }

    pub(super) fn on_conversation_previous_turn(
        &mut self,
        _: &PreviousConversationTurn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.jump_conversation_turn(true, cx);
    }

    pub(super) fn on_conversation_next_turn(
        &mut self,
        _: &NextConversationTurn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.jump_conversation_turn(false, cx);
    }

    pub(super) fn on_rename_thread(
        &mut self,
        _: &RenameThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.close_menu(window, cx);
        if self.window.compact {
            self.toggle_mode(window, cx);
        }
        self.begin_rename(window, cx);
    }

    pub(super) fn on_archive_thread(
        &mut self,
        _: &ArchiveThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        if let Some(meta) = self.current() {
            self.set_archived(meta.id, !meta.archived, cx);
        }
    }

    pub(super) fn on_toggle_thread_pin(
        &mut self,
        _: &ToggleThreadPin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        if let Some(id) = self.selected.id
            && !self.sessions.pending.deleting.contains(&id)
            && self.sessions.entries.iter().any(|session| session.id == id)
        {
            self.toggle_pinned(id, cx);
        }
    }

    pub(super) fn on_toggle_thread_bookmark(
        &mut self,
        _: &ToggleThreadBookmark,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        if let Some(id) = self.selected.id {
            self.toggle_bookmark(id, cx);
        }
    }

    pub(super) fn on_delete_thread(
        &mut self,
        _: &DeleteThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        if let Some(id) = self.selected.id {
            self.request_delete(id, window, cx);
        }
    }

    pub(super) fn on_add_workspace(
        &mut self,
        _: &AddWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.open_workspace_add_menu(window, cx);
    }
}

pub(super) use crate::features::navigation::commands::{
    AddWorkspace, ArchiveThread, DeleteThread, NextConversationTurn, OpenSessionManager,
    PageConversationDown, PageConversationUp, PreviousConversationTurn, RenameThread,
    ScrollConversationToTop, ToggleSession, ToggleThreadBookmark, ToggleThreadPin,
};
