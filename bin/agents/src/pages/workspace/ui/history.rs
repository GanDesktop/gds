//! Conversation integration; discovery and import are hosted above the current workspace.

use super::*;
use crate::shared::i18n::tr;
use bas::model::SessionSnapshot;

impl Workspace {
    pub(super) fn retry_history(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.selected.id else {
            return;
        };
        if self.sessions.runtimes.failure(session).is_some() {
            if self.backend.connected && self.sessions.pending.restarting.insert(session) {
                self.command(Command::RestartSession(session), cx);
                cx.notify();
            }
            return;
        }
        if matches!(
            self.selected.stream,
            StreamState::Loading | StreamState::Live
        ) {
            return;
        }
        self.selected.reload();
        self.command(
            Command::Watch {
                session,
                epoch: self.selected.epoch,
            },
            cx,
        );
        cx.notify();
    }

    pub(super) fn history_retry_label(&self) -> &'static str {
        if self
            .selected
            .id
            .is_some_and(|id| self.sessions.runtimes.failure(id).is_some())
        {
            tr!("session-persistence-retry")
        } else {
            tr!("session-history-retry")
        }
    }

    pub(super) fn history_retry_disabled(&self) -> bool {
        self.selected.id.is_some_and(|id| {
            self.sessions.pending.restarting.contains(&id)
                || (self.sessions.runtimes.failure(id).is_some() && !self.backend.connected)
        })
    }

    pub(super) fn history_request(
        &self,
        agent: String,
        cx: &mut Context<Self>,
    ) -> crate::widgets::history::OpenRequest {
        let owner = cx.entity().downgrade();
        let parent = self.window.handle;
        let server = self.backend.server.clone();
        crate::widgets::history::OpenRequest {
            server: self.backend.server.clone(),
            agents: self.backend.agents.clone(),
            agent: Some(agent),
            cwd: self
                .current()
                .and_then(|meta| meta.cwd.clone())
                .or_else(|| self.navigation.selected_directory.clone()),
            workspaces: self
                .backend
                .workspaces
                .iter()
                .filter(|workspace| !workspace.archived)
                .map(|workspace| workspace.cwd.clone())
                .collect(),
            open_session: std::rc::Rc::new(move |snapshot, cx| {
                if owner.upgrade().is_none() || !cx.windows().contains(&parent) {
                    return false;
                }
                let owner = owner.clone();
                let server = server.clone();
                cx.defer(move |cx| {
                    let host = parent.downcast::<Root>().and_then(|handle| {
                        let root = handle.read(cx).ok()?;
                        crate::shared::lib::diagnostics::content(root, cx)
                            .downcast::<crate::pages::workspace::windows::WorkspaceHost>()
                            .ok()
                    });
                    let _ = parent.update(cx, |_, window, cx| {
                        if let Some(host) = host {
                            if let Err(error) = host
                                .update(cx, |host, cx| host.show_workspace_for(server, window, cx))
                            {
                                tracing::warn!(%error, "cannot return to the history workspace");
                                return;
                            }
                        }
                        let _ = owner.update(cx, |this, cx| {
                            this.adopt_agent_session(snapshot, window, cx);
                            window.activate_window();
                        });
                    });
                });
                true
            }),
        }
    }

    pub(super) fn open_agent_history(
        &mut self,
        agent: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_reply_preview(window, cx);
        self.close_menu(window, cx);
        self.record_settings_target(cx);
        let request = self.history_request(agent, cx);
        let owner = cx.entity().downgrade();
        let parent = self.window.handle;
        cx.defer(move |cx| {
            if !crate::features::navigation::target_page(parent, cx) {
                return;
            }
            if let Err(error) = crate::widgets::history::window::open(request, cx) {
                let _ = owner.update(cx, |this, cx| {
                    this.operation_error(tr!("agent-history-title"), error.to_string(), cx);
                });
            }
        });
    }

    fn adopt_agent_session(
        &mut self,
        snapshot: SessionSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = snapshot.meta.id;
        if self.sessions.removed.contains(&id) {
            return;
        }
        self.sessions.runtimes.observe(&snapshot, false);
        if let Some(meta) = self.sessions.entries.iter_mut().find(|meta| meta.id == id) {
            crate::entities::session::conversation::merge_meta(meta, &snapshot.meta);
        } else {
            self.sessions.entries.insert(0, snapshot.meta);
        }
        self.sessions.record_prompt(id);
        if self.selected.id == Some(id) {
            // The stopped stream has ended. Reattach even when opening the same row,
            // without switching or clearing the user's unsent composition.
            self.selected.reload();
            self.command(
                Command::Watch {
                    session: id,
                    epoch: self.selected.epoch,
                },
                cx,
            );
        } else {
            self.select(id, window, cx);
        }
        cx.notify();
    }

    pub(super) fn agent_session_closed(
        &mut self,
        id: SessionId,
        result: Result<SessionSnapshot, Failure>,
        cx: &mut Context<Self>,
    ) {
        self.sessions.pending.closing.remove(&id);
        match result {
            Ok(snapshot) => {
                self.sessions.runtimes.observe(&snapshot, false);
                if let Some(meta) = self.sessions.entries.iter_mut().find(|meta| meta.id == id) {
                    crate::entities::session::conversation::merge_meta(meta, &snapshot.meta);
                }
                self.selected.timeline.merge_meta(&snapshot.meta);
                if let Some(outbox) = self.sessions.outboxes.get_mut(&id) {
                    outbox.pause();
                }
            }
            Err(error) => self.session_operation_error(
                id,
                tr!("agent-session-close"),
                error.presentation(),
                cx,
            ),
        }
        cx.notify();
    }

    pub(super) fn can_restart_agent_session(&self, id: SessionId) -> bool {
        self.backend.connected
            && self.sessions.healthy(id)
            && !self.sessions.removed.contains(&id)
            && !self.sessions.pending.restarting.contains(&id)
            && !self.sessions.pending.closing.contains(&id)
            && !self.sessions.pending.archiving.contains(&id)
            && !self.sessions.pending.deleting.contains(&id)
            && self.sessions.entries.iter().any(|meta| {
                meta.id == id
                    && meta.status == SessionStatus::Stopped
                    && !meta.archived
                    && (self
                        .backend
                        .agents
                        .iter()
                        .any(|agent| agent.id == meta.agent.as_str() && agent.kind == "embedded")
                        || meta.initialization.as_ref().is_some_and(|init| {
                            init.agent_capabilities.load_session
                                || init
                                    .agent_capabilities
                                    .session_capabilities
                                    .resume
                                    .is_some()
                        }))
            })
    }

    pub(super) fn request_restart_agent_session(&mut self, id: SessionId, cx: &mut Context<Self>) {
        if !self.can_restart_agent_session(id) {
            return;
        }
        self.sessions.pending.restarting.insert(id);
        self.command(Command::RestartSession(id), cx);
        cx.notify();
    }

    pub(super) fn agent_session_restarted(
        &mut self,
        id: SessionId,
        result: Result<SessionSnapshot, Failure>,
        cx: &mut Context<Self>,
    ) {
        if !self.sessions.pending.restarting.remove(&id) {
            return;
        }
        match result {
            Ok(snapshot) => {
                let recovered = self.sessions.runtimes.failure(id).is_some();
                self.sessions.runtimes.observe(&snapshot, false);
                if let Some(meta) = self.sessions.entries.iter_mut().find(|meta| meta.id == id) {
                    crate::entities::session::conversation::merge_meta(meta, &snapshot.meta);
                }
                if self.selected.id == Some(id) {
                    self.selected.timeline.merge_meta(&snapshot.meta);
                    if recovered {
                        self.command(Command::Refresh, cx);
                        return;
                    }
                    self.selected.reload();
                    self.command(
                        Command::Watch {
                            session: id,
                            epoch: self.selected.epoch,
                        },
                        cx,
                    );
                }
            }
            Err(error) => self.session_operation_error(
                id,
                tr!("agent-session-start"),
                error.presentation(),
                cx,
            ),
        }
        cx.notify();
    }

    pub(super) fn request_close_agent_session(&mut self, id: SessionId, cx: &mut Context<Self>) {
        if !self.backend.connected
            || self.sessions.pending.closing.contains(&id)
            || self.sessions.pending.restarting.contains(&id)
            || self.sessions.pending.archiving.contains(&id)
            || self.sessions.pending.deleting.contains(&id)
            || !self.sessions.entries.iter().any(|meta| {
                meta.id == id
                    && !meta.archived
                    && !matches!(meta.status, SessionStatus::Stopped | SessionStatus::Failed)
            })
        {
            return;
        }
        self.sessions.pending.closing.insert(id);
        self.command(Command::CloseSession(id), cx);
        cx.notify();
    }
}
