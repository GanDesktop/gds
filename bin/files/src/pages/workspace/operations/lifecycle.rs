//! Shared operation lifecycle, history preview tracking, affected-tab refresh, and file launch.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

use gds::service::desktop::{Desktop, OpenMode, OpenOutcome};
use tokio_util::sync::CancellationToken;
use zgui::reactive::{on_cleanup_local, spawn_local};

use super::super::{
    navigation::Navigator,
    state::{
        context::{Context, WeakContext},
        model::{Contents, FILES_TASK_KEY},
    },
};
use crate::{
    entities::{file::directory, location::Location},
    features::file_operations::history,
    shared::{i18n, lib::clipboard::Clipboard},
};

const NAME_PREVIEW_LIMIT: usize = 5;

#[derive(Clone, Default)]
struct HistoryPreview {
    undo: Vec<history::Kind>,
    redo: Vec<history::Kind>,
}

struct WindowHistorySlot {
    ctx: WeakContext,
    preview: RefCell<HistoryPreview>,
}

thread_local! {
    static HISTORY_PREVIEWS: RefCell<HashMap<usize, Rc<WindowHistorySlot>>> =
        RefCell::new(HashMap::new());
}

fn history_slot(ctx: &Context) -> Rc<WindowHistorySlot> {
    let key = Rc::as_ptr(&ctx.0) as usize;
    if let Some(existing) = HISTORY_PREVIEWS.with(|map| {
        map.borrow().get(&key).and_then(|slot| {
            slot.ctx
                .upgrade()
                .filter(|existing| Rc::ptr_eq(&existing.0, &ctx.0))
                .map(|_| Rc::clone(slot))
        })
    }) {
        return existing;
    }

    let created = Rc::new(WindowHistorySlot {
        ctx: ctx.downgrade(),
        preview: RefCell::new(HistoryPreview::default()),
    });
    ctx.0.owner.with(|| {
        on_cleanup_local(move || {
            HISTORY_PREVIEWS.with(|map| {
                map.borrow_mut().remove(&key);
            });
        });
    });
    HISTORY_PREVIEWS.with(|map| {
        map.borrow_mut().insert(key, Rc::clone(&created));
    });
    created
}

fn sanitize_preview_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_control() {
            for esc in ch.escape_default() {
                out.push(esc);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

pub(super) fn freeze_name_preview(names: impl IntoIterator<Item = String>) -> (Vec<String>, usize) {
    let mut preview = Vec::with_capacity(NAME_PREVIEW_LIMIT);
    let mut total = 0usize;
    for name in names {
        total += 1;
        if preview.len() < NAME_PREVIEW_LIMIT {
            preview.push(sanitize_preview_name(&name));
        }
    }
    let overflow = total.saturating_sub(preview.len());
    (preview, overflow)
}

pub(super) fn preview_slice(names: Vec<String>) -> (Vec<String>, usize) {
    let total = names.len();
    let overflow = total.saturating_sub(NAME_PREVIEW_LIMIT);
    let items = names.into_iter().take(NAME_PREVIEW_LIMIT).collect();
    (items, overflow)
}

pub(super) fn next_free_remote_name(default_name: &str, reserved: &HashSet<String>) -> String {
    if !reserved.contains(default_name) {
        return default_name.to_owned();
    }
    let mut index = 2usize;
    loop {
        let candidate = format!("{default_name} ({index})");
        if !reserved.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

pub(in crate::pages::workspace) struct Lifecycle<'a>(pub &'a Context);

impl Lifecycle<'_> {
    pub(in crate::pages::workspace) fn record_history(&self, entry: history::Entry) {
        let kind = entry.kind;
        let recorded = self.0.update(|state| {
            let non_empty = !entry.steps.is_empty();
            state.history.record(entry);
            non_empty
        });
        if recorded {
            let slot = history_slot(self.0);
            let mut preview = slot.preview.borrow_mut();
            preview.redo.clear();
            preview.undo.push(kind);
        }
    }

    pub(super) fn clear_history(&self) {
        self.0.update(|state| state.history.clear());
        let slot = history_slot(self.0);
        let mut preview = slot.preview.borrow_mut();
        preview.undo.clear();
        preview.redo.clear();
    }

    pub(super) fn take_history_entry(&self, redo: bool) -> Option<history::Entry> {
        let direction = if redo {
            history::Direction::Redo
        } else {
            history::Direction::Undo
        };
        let entry = self.0.update(|state| state.history.take(direction));
        let slot = history_slot(self.0);
        let mut preview = slot.preview.borrow_mut();
        let stack = if redo {
            &mut preview.redo
        } else {
            &mut preview.undo
        };
        if entry.is_some() {
            let _ = stack.pop();
        } else {
            stack.clear();
        }
        entry
    }

    pub(super) fn finish_history_replay(&self, redo: bool, report: &mut history::Report) {
        let direction = if redo {
            history::Direction::Redo
        } else {
            history::Direction::Undo
        };
        let kind = report.kind;
        let has_remaining = !report.remaining.is_empty();
        let has_inverse = !report.inverse.is_empty();
        self.0.update(|state| {
            state.history.finish(direction, report);
        });
        let finish_slot = history_slot(self.0);
        let mut preview = finish_slot.preview.borrow_mut();
        if has_remaining {
            if redo {
                preview.redo.push(kind);
            } else {
                preview.undo.push(kind);
            }
        }
        if has_inverse {
            if redo {
                preview.undo.push(kind);
            } else {
                preview.redo.push(kind);
            }
        }
    }

    fn peek_history_kind(&self, redo: bool) -> Option<history::Kind> {
        let slot = history_slot(self.0);
        let preview = slot.preview.borrow();
        if redo {
            preview.redo.last().copied()
        } else {
            preview.undo.last().copied()
        }
    }

    pub(in crate::pages::workspace) fn history_available(&self, redo: bool) -> bool {
        !self.0.is_background_task_busy_untracked(FILES_TASK_KEY)
            && !self.0.read_untracked(|state| state.operation.busy())
            && self.peek_history_kind(redo).is_some()
    }

    pub(in crate::pages::workspace) fn history_label(&self, redo: bool) -> String {
        if let Some(kind) = self.peek_history_kind(redo) {
            let op_name = i18n::text_untracked(kind.label_id());
            if redo {
                gds::t_untracked!("redo-operation", operation = op_name)
            } else {
                gds::t_untracked!("undo-operation", operation = op_name)
            }
        } else {
            i18n::text_untracked(if redo { "redo" } else { "undo" })
        }
    }

    pub(in crate::pages::workspace) fn cancel_clipboard_cut(&self) -> bool {
        let Some(snapshot) = Clipboard::current()
            .snapshot_untracked()
            .filter(|snap| snap.can_cancel())
        else {
            return false;
        };
        let paths = snapshot.paths.clone();
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = Clipboard::current().remove(snapshot, paths).await;
                if let Some(ctx) = weak.upgrade() {
                    if let Err(error) = result {
                        ctx.report_notice(error.to_string());
                    }
                    ctx.changed();
                }
            }));
        });
        true
    }

    pub(in crate::pages::workspace) fn refresh_affected(&self, paths: &[PathBuf]) {
        self.refresh_after_file_operation(paths, false);
    }

    pub(super) fn refresh_after_file_operation(&self, paths: &[PathBuf], trash_affected: bool) {
        let mut dir_tabs = Vec::new();
        let mut trash_tabs = Vec::new();
        let mut has_tag_tabs = false;
        let parents: HashSet<PathBuf> = paths
            .iter()
            .filter_map(|path| path.parent().map(Path::to_path_buf))
            .collect();
        self.0.read_untracked(|state| {
            for tab in &state.tabs {
                match tab.contents {
                    Contents::Directory => {
                        if let Some(dir) = tab.navigation.path()
                            && (paths.iter().any(|path| path == dir) || parents.contains(dir))
                        {
                            dir_tabs.push(tab.id);
                        }
                    }
                    Contents::Trash(_) if trash_affected => {
                        trash_tabs.push(tab.id);
                    }
                    Contents::Recent(_) => {
                        dir_tabs.push(tab.id);
                    }
                    Contents::Tags { .. } => {
                        has_tag_tabs = true;
                    }
                    _ => {}
                }
            }
        });
        let navigator = Navigator(self.0);
        for id in dir_tabs {
            navigator.refresh_tab(id);
        }
        for id in trash_tabs {
            navigator.refresh_tab(id);
        }
        if has_tag_tabs {
            navigator.refresh_tag_tabs();
        }
    }

    pub(in crate::pages::workspace) fn begin_operation(
        &self,
        label: &str,
    ) -> Option<CancellationToken> {
        self.0.begin_background_task(FILES_TASK_KEY, label)
    }

    pub(in crate::pages::workspace) fn end_operation(&self) {
        let _ = self.0.finish_background_task(FILES_TASK_KEY);
    }

    pub(in crate::pages::workspace) fn open_file(&self, path: PathBuf, mode: OpenMode) {
        let cancel = self.0.0.tasks.token();
        let record_path = path.clone();
        self.0.work(
            "Open file",
            async move {
                let desktop = Desktop::default();
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => Ok(OpenOutcome::Cancelled),
                    result = desktop.open_file(&path, mode) => {
                        result.map_err(|error| error.to_string())
                    }
                }
            },
            move |ctx, result| match result {
                Ok(OpenOutcome::Opened) => Lifecycle(ctx).record_recent_file(record_path),
                Ok(OpenOutcome::Cancelled) => {}
                Err(error) => ctx.report_notice(error),
            },
        );
    }

    fn record_recent_file(&self, path: PathBuf) {
        let mime = directory::Entry::inspect(&path).ok().and_then(|entry| {
            entry
                .details
                .display_content()
                .and_then(|c| c.mime.as_ref().map(|m| m.to_string()))
        });
        self.0.blocking(
            "Record recent file",
            move || {
                if let Err(error) = gds::service::desktop::recent_files::record_default(
                    &path,
                    mime.as_deref(),
                    "GDS Files",
                    "'gds-files' %u",
                ) {
                    tracing::debug!(%error, "failed to record recently-used.xbel entry");
                }
                Ok(())
            },
            |ctx, _| {
                Navigator(ctx).refresh_location(&Location::Recent);
            },
        );
    }
}
