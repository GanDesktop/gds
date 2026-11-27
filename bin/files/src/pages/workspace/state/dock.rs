//! Window-owned terminal dock state independent of browser navigation or UI controllers.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use gds::{app::Gds, i18n, service::terminal::Status, t, ui::terminal::Terminal};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::view::{FrameHandle, ListenerGuard};

pub(in crate::pages::workspace) const MIN_DOCK_HEIGHT: f32 = 160.0;
pub(in crate::pages::workspace) const DEFAULT_DOCK_HEIGHT: f32 = 280.0;
pub(in crate::pages::workspace) const MIN_CONTENT_HEIGHT: f32 = 160.0;

#[derive(Clone)]
pub(in crate::pages::workspace) struct SessionEntry {
    pub(in crate::pages::workspace) id: u64,
    pub(in crate::pages::workspace) terminal: Terminal,
    pub(in crate::pages::workspace) view_ref: NodeRef,
    pub(in crate::pages::workspace) started_in: PathBuf,
    pub(in crate::pages::workspace) custom_title: RwSignal<Option<String>, LocalStorage>,
    pub(in crate::pages::workspace) closing: Cell<bool>,
}

impl SessionEntry {
    pub(in crate::pages::workspace) fn fallback_title(&self) -> String {
        self.started_in
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.started_in.display().to_string())
    }

    pub(in crate::pages::workspace) fn display_title(&self) -> String {
        if let Some(custom) = self.custom_title.try_get().flatten()
            && !custom.trim().is_empty()
        {
            return custom;
        }
        if let Some(pty_title) = self.terminal.title().try_get().flatten()
            && !pty_title.trim().is_empty()
        {
            return pty_title;
        }
        self.fallback_title()
    }

    pub(in crate::pages::workspace) fn display_title_untracked(&self) -> String {
        if let Some(custom) = self.custom_title.try_get_untracked().flatten()
            && !custom.trim().is_empty()
        {
            return custom;
        }
        if let Some(pty_title) = self.terminal.title().try_get_untracked().flatten()
            && !pty_title.trim().is_empty()
        {
            return pty_title;
        }
        self.fallback_title()
    }

    pub(in crate::pages::workspace) fn detail_lines(&self) -> Vec<String> {
        let origin = t!(
            "files-terminal-started-in",
            path = self.started_in.display().to_string()
        );
        let status = match self.terminal.status().try_get().unwrap_or(Status::Closed) {
            Status::Starting => i18n::text("gds-terminal-starting"),
            Status::Running => i18n::text("gds-terminal-running"),
            Status::Stopping => i18n::text("gds-terminal-stopping"),
            Status::Closed => i18n::text("gds-terminal-exited"),
            Status::Exited { code, signal } => t!(
                "gds-terminal-exit-status",
                code = i64::from(code),
                signal = signal.unwrap_or_else(|| "—".to_owned())
            ),
            Status::Failed(error) => error,
        };
        vec![origin, status]
    }
}

#[derive(Clone)]
pub(in crate::pages::workspace) struct RenameDialogState {
    pub(in crate::pages::workspace) session_id: u64,
    pub(in crate::pages::workspace) draft: RwSignal<String, LocalStorage>,
    pub(in crate::pages::workspace) input_ref: NodeRef,
}

#[derive(Clone)]
pub(in crate::pages::workspace) struct InternalFileDrag {
    pub(in crate::pages::workspace) start: (f32, f32),
    pub(in crate::pages::workspace) active: bool,
    pub(in crate::pages::workspace) paths: Vec<PathBuf>,
}

pub(in crate::pages::workspace) struct DockInner {
    pub(in crate::pages::workspace) revision: RwSignal<u64, LocalStorage>,
    pub(in crate::pages::workspace) next_id: Cell<u64>,
    pub(in crate::pages::workspace) sealed: Cell<bool>,
    pub(in crate::pages::workspace) sessions: RefCell<Vec<SessionEntry>>,
    pub(in crate::pages::workspace) maximized: RwSignal<bool, LocalStorage>,
    pub(in crate::pages::workspace) height: RwSignal<f32, LocalStorage>,
    pub(in crate::pages::workspace) workspace_size: RwSignal<(f32, f32), LocalStorage>,
    pub(in crate::pages::workspace) dragging: RwSignal<Option<(f32, f32)>, LocalStorage>,
    pub(in crate::pages::workspace) dock_node: NodeRef,
    pub(in crate::pages::workspace) rail_node: NodeRef,
    pub(in crate::pages::workspace) pane_node: NodeRef,
    pub(in crate::pages::workspace) rename_dialog:
        RwSignal<Option<RenameDialogState>, LocalStorage>,
    pub(in crate::pages::workspace) pending_rename_focus: RefCell<Option<FrameHandle>>,
    pub(in crate::pages::workspace) internal_drag: RefCell<Option<InternalFileDrag>>,
    pub(in crate::pages::workspace) internal_drop_hover: RwSignal<bool, LocalStorage>,
    pub(in crate::pages::workspace) drag_listeners: RefCell<Vec<ListenerGuard>>,
}

impl Drop for DockInner {
    fn drop(&mut self) {
        self.sealed.set(true);
        let drained: Vec<SessionEntry> = self.sessions.borrow_mut().drain(..).collect();
        if drained.is_empty() {
            return;
        }
        let shutdowns: Vec<_> = drained
            .into_iter()
            .map(|entry| entry.terminal.shutdown())
            .collect();
        if let Some(gds) = Gds::try_current() {
            gds.tasks()
                .scope()
                .spawn_named("Shut down window terminals", async move {
                    for task in shutdowns {
                        task.await;
                    }
                });
        }
    }
}

#[derive(Clone)]
pub(in crate::pages::workspace) struct Dock {
    pub(in crate::pages::workspace) active: RwSignal<Option<u64>, LocalStorage>,
    pub(in crate::pages::workspace) inner: Rc<DockInner>,
}

impl Dock {
    pub(in crate::pages::workspace) fn new() -> Self {
        Self {
            active: RwSignal::new_local(None),
            inner: Rc::new(DockInner {
                revision: RwSignal::new_local(0),
                next_id: Cell::new(1),
                sealed: Cell::new(false),
                sessions: RefCell::new(Vec::new()),
                maximized: RwSignal::new_local(false),
                height: RwSignal::new_local(DEFAULT_DOCK_HEIGHT),
                workspace_size: RwSignal::new_local((1024.0, 640.0)),
                dragging: RwSignal::new_local(None),
                dock_node: NodeRef::new(),
                rail_node: NodeRef::new(),
                pane_node: NodeRef::new(),
                rename_dialog: RwSignal::new_local(None),
                pending_rename_focus: RefCell::new(None),
                internal_drag: RefCell::new(None),
                internal_drop_hover: RwSignal::new_local(false),
                drag_listeners: RefCell::new(Vec::new()),
            }),
        }
    }

    pub(in crate::pages::workspace) fn bump(&self) {
        let _ = self
            .inner
            .revision
            .try_update(|rev| *rev = rev.wrapping_add(1));
    }

    pub(in crate::pages::workspace) fn clamped_height(&self, raw: f32) -> f32 {
        let available = self
            .inner
            .workspace_size
            .try_get_untracked()
            .map_or(640.0, |(_, h)| h);
        let max_height = (available - MIN_CONTENT_HEIGHT).max(MIN_DOCK_HEIGHT);
        raw.clamp(MIN_DOCK_HEIGHT, max_height)
    }

    pub(in crate::pages::workspace) fn base_height(&self) -> f32 {
        let available = self
            .inner
            .workspace_size
            .try_get()
            .map_or(640.0, |(_, h)| h);
        let max_height = (available - MIN_CONTENT_HEIGHT).max(MIN_DOCK_HEIGHT);
        self.inner
            .height
            .try_get()
            .unwrap_or(DEFAULT_DOCK_HEIGHT)
            .clamp(MIN_DOCK_HEIGHT, max_height)
    }

    pub(in crate::pages::workspace) fn maximized_height(&self) -> f32 {
        let available = self
            .inner
            .workspace_size
            .try_get()
            .map_or(640.0, |(_, h)| h);
        available.max(MIN_DOCK_HEIGHT)
    }

    pub(in crate::pages::workspace) fn find_session(&self, id: u64) -> Option<SessionEntry> {
        self.inner
            .sessions
            .borrow()
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
    }

    pub(in crate::pages::workspace) fn session_snapshot(&self) -> Vec<SessionEntry> {
        self.inner.sessions.borrow().clone()
    }

    pub(in crate::pages::workspace) fn cancel_drag_hover(&self) -> bool {
        let was_active = self
            .inner
            .internal_drag
            .borrow_mut()
            .take()
            .is_some_and(|drag| drag.active);
        if self
            .inner
            .internal_drop_hover
            .try_get_untracked()
            .unwrap_or(false)
        {
            let _ = self.inner.internal_drop_hover.try_set(false);
        }
        was_active
    }

    pub(in crate::pages::workspace) fn shutdown_all(&self) {
        if self.inner.sealed.replace(true) {
            return;
        }
        *self.inner.pending_rename_focus.borrow_mut() = None;
        self.inner.drag_listeners.borrow_mut().clear();
        let drained: Vec<SessionEntry> = self.inner.sessions.borrow_mut().drain(..).collect();
        if drained.is_empty() {
            return;
        }
        let shutdowns: Vec<_> = drained
            .into_iter()
            .map(|entry| {
                entry.closing.set(true);
                entry.terminal.shutdown()
            })
            .collect();
        if let Some(gds) = Gds::try_current() {
            gds.tasks()
                .scope()
                .spawn_named("Shut down window terminals", async move {
                    for task in shutdowns {
                        task.await;
                    }
                });
        }
    }
}
