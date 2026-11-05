//! Directory and thread navigation, sidebar presentation and persisted selection.

use crate::pages::workspace::ui::*;
use crate::shared::i18n::tr;

mod controls;
mod directories;
pub(super) mod draft_row;
mod folders;
pub(super) mod groups;
pub(super) mod history;
pub(super) mod restore;
mod sidebar;
pub(super) mod thread_row;
mod workspaces;

pub(super) struct CycleTargets {
    current: Option<SessionId>,
    next: Option<SessionId>,
    previous: Option<SessionId>,
}

impl CycleTargets {
    fn new(threads: &[SessionId], current: Option<SessionId>) -> Self {
        let index = threads.iter().position(|id| Some(*id) == current);
        let (next, previous) = if threads.is_empty() {
            (None, None)
        } else {
            let next = index.map_or(0, |index| (index + 1) % threads.len());
            let previous = index.map_or(threads.len() - 1, |index| {
                (index + threads.len() - 1) % threads.len()
            });
            (Some(threads[next]), Some(threads[previous]))
        };
        Self {
            current,
            next,
            previous,
        }
    }

    pub(super) fn hints(&self, session: SessionId, compact: bool) -> Vec<AnyElement> {
        if self.current == Some(session) {
            return Vec::new();
        }
        let mut hints = Vec::new();
        for (target, name, reverse) in [
            (self.next, "next", false),
            (self.previous, "previous", true),
        ] {
            if target != Some(session) {
                continue;
            }
            let action: Box<dyn gpui_kit::Action> = if reverse {
                Box::new(PreviousThread)
            } else {
                Box::new(NextThread)
            };
            let hint = gds::ui::keytips::Anchor::new(
                SharedString::from(format!("thread-{name}-{session}")),
                gds::ui::Shortcut::new(action, Some("BasWorkspace")),
            );
            let hint = if compact {
                hint.bottom_corner()
            } else {
                hint.trailing_before(px(gds::appearance::tokens::ICON_BUTTON_SIZE
                    + gds::appearance::tokens::SIDEBAR_PADDING))
            };
            hints.push(hint.into_any_element());
        }
        hints
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum View {
    #[default]
    Workspaces,
    Bookmarks,
    History,
    Archived,
}

impl View {
    pub(super) const ALL: [Self; 4] = [
        Self::Workspaces,
        Self::Bookmarks,
        Self::History,
        Self::Archived,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Workspaces => tr!("sidebar-workspaces"),
            Self::Bookmarks => tr!("sidebar-bookmarks"),
            Self::History => tr!("sidebar-history"),
            Self::Archived => tr!("sidebar-archived"),
        }
    }

    pub(super) fn icon(self) -> IconName {
        match self {
            Self::Workspaces => IconName::MessagesSquare,
            Self::Bookmarks => IconName::Bookmark,
            Self::History => IconName::Clock,
            Self::Archived => IconName::Archive,
        }
    }

    pub(super) fn includes(self, session: &SessionMeta, bookmarks: &HashSet<SessionId>) -> bool {
        match self {
            Self::Bookmarks => bookmarks.contains(&session.id),
            Self::Archived => session.archived,
            Self::Workspaces | Self::History => !session.archived,
        }
    }

    pub(super) fn flat(self) -> bool {
        matches!(self, Self::Bookmarks | Self::History)
    }
}

impl Workspace {
    pub(super) fn pinned_sessions(&self) -> Vec<SessionMeta> {
        self.sessions
            .entries
            .iter()
            .filter(|session| !session.archived && self.navigation.pinned.contains(&session.id))
            .cloned()
            .collect()
    }

    pub(super) fn sidebar_sessions(&self) -> Vec<SessionMeta> {
        let workspace = self
            .workspace_for_directory(&self.navigation.selected_directory)
            .map(|workspace| workspace.id);
        self.visible_sessions()
            .into_iter()
            .filter(|session| Some(session.workspace) == workspace)
            .filter(|session| session.archived || !self.navigation.pinned.contains(&session.id))
            .collect()
    }

    pub(super) fn toggle_bookmark(&mut self, session: SessionId, cx: &mut Context<Self>) {
        if self.sessions.pending.deleting.contains(&session)
            || !self.sessions.entries.iter().any(|meta| meta.id == session)
        {
            return;
        }
        let bookmarked = !self.navigation.bookmarks.contains(&session);
        if let Some(store) = &mut self.navigation.restore.store {
            if let Err(error) = store.set_bookmarked(session, bookmarked) {
                self.session_operation_error(
                    session,
                    tr!("session-bookmark-failed"),
                    error.to_string(),
                    cx,
                );
                return;
            }
            self.navigation.bookmarks = store.bookmarks();
        } else if bookmarked {
            self.navigation.bookmarks.insert(session);
        } else {
            self.navigation.bookmarks.remove(&session);
        }
        cx.notify();
    }

    pub(super) fn grouped_threads<'a>(
        &self,
        sessions: &'a [SessionMeta],
        query: &str,
    ) -> Vec<groups::Group<'a>> {
        let workspaces: Vec<_> = self
            .backend
            .workspaces
            .iter()
            .filter(|workspace| {
                if self.navigation.view == View::Archived {
                    workspace.archived
                        || sessions
                            .iter()
                            .any(|session| session.workspace == workspace.id)
                } else {
                    !workspace.archived
                }
            })
            .cloned()
            .collect();
        let mut groups = groups::group_threads(sessions, &workspaces, query);
        for group in &mut groups {
            group
                .threads
                .sort_by_key(|session| !self.navigation.pinned.contains(&session.id));
        }
        groups
    }

    pub(super) fn toggle_pinned(&mut self, session: SessionId, cx: &mut Context<Self>) {
        let pinned = !self.navigation.pinned.contains(&session);
        if let Some(store) = &mut self.navigation.restore.store {
            if let Err(error) = store.set_pinned(session, pinned) {
                self.session_operation_error(
                    session,
                    tr!("session-pin-failed"),
                    error.to_string(),
                    cx,
                );
                return;
            }
            self.navigation.pinned = store.pinned();
        } else if pinned {
            self.navigation.pinned.insert(session);
        } else {
            self.navigation.pinned.remove(&session);
        }
        cx.notify();
    }

    pub(super) fn cycle_thread(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let targets = self.thread_cycle_targets(cx);
        let target = if reverse {
            targets.previous
        } else {
            targets.next
        };
        let Some(target) = target else { return };
        self.close_menu(window, cx);
        self.select(target, window, cx);
    }

    pub(super) fn sidebar_session_ids(&self, query: &str) -> Vec<SessionId> {
        let pinned = self.pinned_sessions();
        let sessions = self.sidebar_sessions();
        history::sorted(&pinned)
            .into_iter()
            .chain(history::sorted(&sessions))
            .filter(|session| matches_query(session, query))
            .map(|session| session.id)
            .collect()
    }

    pub(super) fn thread_cycle_targets(&self, cx: &App) -> CycleTargets {
        let query = self.navigation.search.read(cx).value().to_lowercase();
        CycleTargets::new(&self.sidebar_session_ids(&query), self.selected.id)
    }
}
