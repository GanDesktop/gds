use super::groups;
use crate::pages::workspace::ui::*;
use crate::shared::i18n::tr;
use connection::WorkspaceMutation;

impl Workspace {
    pub(in crate::pages::workspace) fn workspace_blocked(
        &self,
        id: bas::model::WorkspaceId,
    ) -> bool {
        self.navigation.workspace_pending.contains(&id)
            || self.backend.removed_workspaces.contains(&id)
            || self
                .backend
                .workspaces
                .iter()
                .any(|workspace| workspace.id == id && workspace.archived)
    }

    pub(in crate::pages::workspace) fn workspace_for_directory(
        &self,
        directory: &Option<PathBuf>,
    ) -> Option<&bas::model::Workspace> {
        self.backend
            .workspaces
            .iter()
            .find(|workspace| Some(&workspace.cwd) == directory.as_ref())
    }

    pub(in crate::pages::workspace) fn merge_workspaces(
        &mut self,
        mut incoming: Vec<bas::model::Workspace>,
    ) {
        incoming.retain(|workspace| !self.backend.removed_workspaces.contains(&workspace.id));
        let ids: HashSet<_> = incoming.iter().map(|workspace| workspace.id).collect();
        for old in &self.backend.workspaces {
            if let Some(workspace) = incoming.iter_mut().find(|workspace| workspace.id == old.id) {
                if old.revision > workspace.revision {
                    *workspace = old.clone();
                }
            } else if !self.backend.observed_workspaces.contains(&old.id)
                && !self.backend.removed_workspaces.contains(&old.id)
            {
                incoming.push(old.clone());
            }
        }
        self.backend
            .removed_workspaces
            .extend(self.backend.observed_workspaces.difference(&ids).copied());
        self.backend.observed_workspaces = ids;
        self.backend.workspaces = incoming;
    }

    pub(in crate::pages::workspace) fn can_remove_workspace(
        &self,
        directory: &Option<PathBuf>,
    ) -> bool {
        self.backend.connected
            && self
                .workspace_for_directory(directory)
                .is_some_and(|workspace| !self.navigation.workspace_pending.contains(&workspace.id))
            && !(self.selected.creating && self.navigation.selected_directory == *directory)
    }

    pub(in crate::pages::workspace) fn request_remove_workspace(
        &mut self,
        directory: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if !self.can_remove_workspace(&directory) {
            return;
        }
        let Some(workspace) = self.workspace_for_directory(&directory) else {
            return;
        };
        let id = workspace.id;
        let count = self
            .sessions
            .entries
            .iter()
            .filter(|session| session.workspace == id)
            .count();
        let message = if count == 0 {
            tr!(
                "workspace-delete-confirm",
                path = groups::path_label(directory.as_deref())
            )
        } else {
            tr!(
                "workspace-delete-cascade-confirm",
                path = groups::path_label(directory.as_deref()),
                count = count
            )
        };
        self.confirm_workspace_mutation(
            WorkspaceMutation::Delete {
                id,
                force: count > 0,
            },
            tr!("workspace-delete"),
            message,
            cx,
        );
    }

    pub(in crate::pages::workspace) fn request_archive_workspace(
        &mut self,
        directory: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if !self.can_remove_workspace(&directory) {
            return;
        }
        let Some(workspace) = self.workspace_for_directory(&directory) else {
            return;
        };
        let mutation = WorkspaceMutation::Archive {
            id: workspace.id,
            archived: !workspace.archived,
        };
        if workspace.archived {
            self.mutate_workspace(mutation, cx);
        } else {
            self.confirm_workspace_mutation(
                mutation,
                tr!("workspace-archive"),
                tr!(
                    "workspace-archive-confirm",
                    path = groups::path_label(directory.as_deref())
                ),
                cx,
            );
        }
    }

    fn confirm_workspace_mutation(
        &mut self,
        mutation: WorkspaceMutation,
        label: &'static str,
        message: String,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let request = crate::shared::ui::dialogs::Confirmation {
            title: label.into(),
            message,
            confirm_label: label.into(),
            destructive: true,
        };
        if let Err(error) = crate::shared::ui::dialogs::open_confirmation(
            request,
            move |cx| {
                let _ = owner.update(cx, |this, cx| this.mutate_workspace(mutation, cx));
            },
            cx,
        ) {
            self.operation_error(tr!("workspace-update-failed"), error.to_string(), cx);
        }
    }

    pub(in crate::pages::workspace) fn mutate_workspace(
        &mut self,
        mutation: WorkspaceMutation,
        cx: &mut Context<Self>,
    ) {
        match &mutation {
            WorkspaceMutation::Archive { id, .. } | WorkspaceMutation::Delete { id, .. } => {
                if !self.backend.connected || !self.navigation.workspace_pending.insert(*id) {
                    return;
                }
            }
            WorkspaceMutation::Create { .. } => self.navigation.picker.pending = true,
        }
        self.command(Command::Workspace(mutation), cx);
        cx.notify();
    }

    pub(in crate::pages::workspace) fn finish_workspace_operation(
        &mut self,
        mutation: &WorkspaceMutation,
    ) {
        match mutation {
            WorkspaceMutation::Archive { id, .. } | WorkspaceMutation::Delete { id, .. } => {
                self.navigation.workspace_pending.remove(id);
            }
            WorkspaceMutation::Create { .. } => self.navigation.picker.pending = false,
        }
    }

    pub(in crate::pages::workspace) fn workspace_completed(
        &mut self,
        mutation: WorkspaceMutation,
        result: Result<Option<bas::model::Workspace>, Failure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_workspace_operation(&mutation);
        match result {
            Err(error) => {
                self.operation_error(tr!("workspace-update-failed"), error.presentation(), cx)
            }
            Ok(workspace) => {
                if let Some(workspace) = workspace {
                    let directory = Some(workspace.cwd.clone());
                    let archived = workspace.archived;
                    let id = workspace.id;
                    if !self.backend.removed_workspaces.contains(&id) {
                        if let Some(old) =
                            self.backend.workspaces.iter_mut().find(|old| old.id == id)
                        {
                            if workspace.revision >= old.revision {
                                *old = workspace;
                            }
                        } else {
                            self.backend.workspaces.push(workspace);
                        }
                    }
                    if archived {
                        for session in self
                            .sessions
                            .entries
                            .iter()
                            .filter(|session| session.workspace == id)
                        {
                            if let Some(outbox) = self.sessions.outboxes.get_mut(&session.id) {
                                outbox.pause();
                            }
                        }
                        if self.navigation.selected_directory == directory {
                            self.detach_thread(None, window, cx);
                            self.navigation.session_requested = false;
                            self.composer.switch_target(Draft::default(), window, cx);
                        }
                        if matches!(mutation, WorkspaceMutation::Create { .. }) {
                            self.navigation.view = super::View::Archived;
                        }
                    } else if matches!(mutation, WorkspaceMutation::Create { .. }) {
                        self.detach_thread(directory, window, cx);
                        self.navigation.session_requested = false;
                        self.composer.switch_target(Draft::default(), window, cx);
                        self.focus_input(window, cx);
                    }
                }
                if let WorkspaceMutation::Delete { id, .. } = mutation {
                    self.backend.removed_workspaces.insert(id);
                    let directory = self
                        .backend
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.id == id)
                        .map(|workspace| workspace.cwd.clone());
                    self.backend
                        .workspaces
                        .retain(|workspace| workspace.id != id);
                    let sessions: Vec<_> = self
                        .sessions
                        .entries
                        .iter()
                        .filter(|session| session.workspace == id)
                        .map(|session| session.id)
                        .collect();
                    self.sessions_removed(&sessions, window, cx);
                    self.navigation.drafts.remove(&directory);
                    self.navigation.collapsed.remove(&directory);
                    if self.navigation.selected_directory == directory {
                        self.detach_thread(None, window, cx);
                        self.navigation.session_requested = false;
                        self.composer.switch_target(Draft::default(), window, cx);
                    }
                }
            }
        }
        // A failed cascade may have completed some children; refresh on both outcomes.
        self.command(Command::Refresh, cx);
        cx.notify();
    }
}
