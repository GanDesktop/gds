//! Coalesce filesystem events and reconcile entries without restarting navigation.
mod presentation;
use tokio_util::sync::CancellationToken;

use super::*;

const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

struct WatchTarget {
    revision: u64,
    snapshot: Arc<directory::DirectorySnapshot>,
}

enum WatchState {
    Loading,
    Ready(Option<WatchTarget>),
    Closed,
}

enum WatchEvent {
    Inspect(tokio::sync::oneshot::Sender<WatchState>),
    Apply {
        revision: u64,
        result: Result<Arc<directory::DirectorySnapshot>, directory::Error>,
    },
}

pub(in crate::pages::workspace) struct Watch {
    path: PathBuf,
    cancel: CancellationToken,
    _events: Task<()>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn watch_directory(
        &mut self,
        id: u64,
        path: Option<&Path>,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.tab_slot_mut(id) else {
            return;
        };
        if slot.watch().map(|watch| watch.path.as_path()) == path {
            return;
        }
        slot.set_watch(None);
        let Some(path) = path else { return };
        let mut watcher = match directory::Watching::new(path) {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "cannot watch directory");
                return;
            }
        };
        let tasks = self.window.tasks.clone();
        let worker_tasks = tasks.clone();
        let cancel = tasks.token().child_token();
        let worker_cancel = cancel.clone();
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(1);
        let watcher_task = tasks
            .spawn_cancellable_named("Watch directory", async move {
                while watcher.changed().await {
                    tokio::time::sleep(DEBOUNCE).await;
                    if worker_cancel.is_cancelled() {
                        return;
                    }
                    // Keep changes queued during navigation rather than scheduling another
                    // full refresh, which can otherwise loop during sustained downloads.
                    let target = loop {
                        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                        if events_tx.send(WatchEvent::Inspect(reply_tx)).await.is_err() {
                            return;
                        }
                        match reply_rx.await {
                            Ok(WatchState::Loading) => tokio::time::sleep(DEBOUNCE).await,
                            Ok(WatchState::Ready(target)) => break target,
                            Ok(WatchState::Closed) | Err(_) => return,
                        }
                    };
                    let changes = watcher.take_changes();
                    let Some(WatchTarget { revision, snapshot }) = target else {
                        continue;
                    };
                    let apply_cancel = worker_cancel.clone();
                    let work = worker_tasks
                        .spawn_blocking_named("Reconcile directory changes", move || {
                            changes.apply_shared(&snapshot, &apply_cancel)
                        });
                    let result = match work
                        .await
                        .map_err(directory::Error::from)
                        .and_then(std::convert::identity)
                    {
                        Ok(Some(snapshot)) => Ok(snapshot),
                        Ok(None) => continue,
                        Err(error) if error.is_cancelled() => return,
                        Err(error) => Err(error),
                    };
                    if events_tx
                        .send(WatchEvent::Apply { revision, result })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .cancel_on_drop();
        let events = cx.spawn(async move |this, cx| {
            let _watcher = watcher_task;
            while let Some(event) = events_rx.recv().await {
                match event {
                    WatchEvent::Inspect(reply) => {
                        let state = this
                            .update(cx, |this, _| {
                                let Some(slot) = this.tab_slot(id) else {
                                    return WatchState::Closed;
                                };
                                if slot.is_loading() {
                                    return WatchState::Loading;
                                }
                                WatchState::Ready(slot.navigation.directory_arc().map(|snapshot| {
                                    WatchTarget {
                                        revision: slot.navigation.revision(),
                                        snapshot,
                                    }
                                }))
                            })
                            .unwrap_or(WatchState::Closed);
                        let _ = reply.send(state);
                    }
                    WatchEvent::Apply { revision, result } => {
                        if this
                            .update(cx, |this, cx| match result {
                                Ok(snapshot) => {
                                    this.apply_watch_snapshot(id, revision, snapshot, cx)
                                }
                                Err(error) => {
                                    tracing::warn!(%error, "cannot reconcile directory changes")
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
        if let Some(slot) = self.tab_slot_mut(id) {
            slot.set_watch(Some(Watch {
                path: path.to_owned(),
                cancel,
                _events: events,
            }));
        }
    }

    fn apply_watch_snapshot(
        &mut self,
        id: u64,
        revision: u64,
        snapshot: Arc<directory::DirectorySnapshot>,
        cx: &mut Context<Self>,
    ) {
        if id != self.tabs.active_id {
            if let Some(slot) = self.tab_slot_mut(id)
                && slot.navigation.update_directory(revision, snapshot)
            {
                slot.projection_dirty = true;
                slot.set_local_enrich(None);
            }
            return;
        }
        let Some(slot) = self.tab_slot(id) else {
            return;
        };
        let previous = slot.table_rows.clone();
        let selected = slot.navigation.view().selected.clone();
        let layout = self.columns_layout(&slot.location()).clone();
        let active = self.pages.is_empty();
        let details = self.details_visible();
        let Some(slot) = self.tab_slot_mut(id) else {
            return;
        };
        if !slot.navigation.update_directory(revision, snapshot) {
            return;
        }
        slot.set_local_enrich(None);
        slot.refresh_visible();
        let rows = slot.table_rows.clone();
        let changed = active
            && (selected != slot.navigation.view().selected
                || presentation::changed(slot, &previous, &layout, details, cx));
        let notify_browser = changed
            && (slot.view_mode == ViewMode::Grid
                || details
                || !selected.is_empty()
                || previous
                    .iter()
                    .map(|entry| &entry.path)
                    .ne(rows.iter().map(|entry| &entry.path)));
        // Keep input handlers and later scrolling current even when no pixels change.
        table::FileTable::update_rows(&self.files.table, rows, changed, cx);
        self.schedule_enrichment(id, cx);
        if notify_browser {
            cx.notify();
        }
    }
}
