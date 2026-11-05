use super::*;
use crate::shared::i18n::tr;

impl Workspace {
    /// Both a successful delete and disappearance from a serial catalog confirm removal.
    pub(super) fn sessions_removed(
        &mut self,
        removed: &[SessionId],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace_unavailable = self
            .workspace_for_directory(&self.navigation.selected_directory)
            .is_none_or(|workspace| workspace.archived);
        // Invalidate the whole batch before selection can commit an outgoing title or draft.
        for &session in removed {
            self.sessions.mark_removed(session);
            self.sessions.entries.retain(|meta| meta.id != session);
            self.sessions.observed.remove(&session);
            self.sessions.discard_content(session);
            self.sessions.pending.forget(session);
            self.sessions.seen_replies.remove(&session);
            self.navigation.marker_bookmarks.remove(&session);
            self.renaming.forget(session);
            self.command(Command::ForgetHistory(session), cx);
        }
        self.forget_read_state(removed, cx);
        if self.selected.id.is_some_and(|id| removed.contains(&id)) {
            if let Some(next) = self
                .sessions
                .entries
                .iter()
                .find(|meta| !meta.archived)
                .map(|meta| meta.id)
            {
                self.select(next, window, cx);
            } else if workspace_unavailable {
                self.detach_thread(None, window, cx);
                self.navigation.session_requested = false;
                self.composer.switch_target(Draft::default(), window, cx);
            } else {
                self.start_agent_chat(self.selected.agent.clone(), window, cx);
            }
        }
        for &session in removed {
            if self.navigation.pinned.contains(&session) {
                self.toggle_pinned(session, cx);
            }
        }
    }

    pub(super) fn discard_session_draft(
        &mut self,
        session: SessionId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sessions.is_draft(session)
            || self.sessions.pending.archiving.contains(&session)
            || self.sessions.pending.configuring.contains(&session)
            || self.sessions.pending.loading_attachments.contains(&session)
            || (self.selected.id == Some(session) && self.composer.paste_pending)
            || self
                .sessions
                .outboxes
                .get(&session)
                .is_some_and(outbox::SessionOutbox::pending)
            || self.sessions.entries.iter().any(|meta| {
                meta.id == session
                    && (meta.archived
                        || meta.status == SessionStatus::Working
                        || meta.active_turn.is_some())
            })
        {
            return;
        }
        self.delete_thread(session, cx);
    }

    fn delete_thread(&mut self, session: SessionId, cx: &mut Context<Self>) {
        if !self.backend.connected
            || !self.sessions.entries.iter().any(|meta| meta.id == session)
            || !self.sessions.pending.deleting.insert(session)
        {
            return;
        }
        self.command(Command::Delete(session), cx);
        cx.notify();
    }

    pub(super) fn mark_thread_read(&mut self, session: SessionId, cx: &mut Context<Self>) {
        let cursor = self
            .sessions
            .entries
            .iter()
            .find(|meta| meta.id == session)
            .map(|meta| meta.last_reply.raw())
            .unwrap_or(0);
        let replayed = self
            .selected
            .timeline
            .conversation
            .as_ref()
            .filter(|conversation| conversation.session.id == session)
            .and_then(|conversation| conversation.last_reply)
            .map_or(0, |reply| reply.cursor);
        self.see_reply(session, cursor.max(replayed), cx);
        self.persist_read_state(cx);
        cx.notify();
    }

    pub(super) fn operation_error(&mut self, title: &str, message: String, cx: &mut Context<Self>) {
        self.selected.error = Some(message.clone());
        self.report_operation_error(title, &message, cx);
        cx.notify();
    }

    pub(super) fn report_operation_error(&self, title: &str, message: &str, cx: &mut App) {
        use crate::shared::lib::diagnostics::{self, log::Level};
        if self.window.presented {
            diagnostics::notify(Some(self.window.handle), Level::Error, title, message, cx);
        } else {
            diagnostics::record(Level::Error, title, message, cx);
        }
    }

    pub(super) fn request_delete(
        &mut self,
        session: SessionId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sessions.pending.deleting.contains(&session) || !self.backend.connected {
            return;
        }
        let Some(meta) = self.sessions.entries.iter().find(|meta| meta.id == session) else {
            return;
        };
        let confirmation = crate::shared::ui::dialogs::Confirmation {
            title: tr!("session-delete-title").into(),
            message: tr!("session-delete-confirm", title = (title(meta)).to_string()),
            confirm_label: tr!("action-delete-permanently").into(),
            destructive: true,
        };
        let view = cx.entity().downgrade();
        if let Err(error) = crate::features::delete_session::confirm(
            confirmation,
            move |cx| {
                let _ = view.update(cx, |this, cx| {
                    this.delete_thread(session, cx);
                });
            },
            cx,
        ) {
            self.session_operation_error(
                session,
                tr!("session-delete-confirm-failed"),
                error.to_string(),
                cx,
            );
        }
    }
}
