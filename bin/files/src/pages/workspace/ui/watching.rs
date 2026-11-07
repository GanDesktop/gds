//! Coalesce filesystem events and reconcile entries without restarting navigation.
mod changes;
mod presentation;
use super::*;
use changes::Changes;
use notify::{EventKind, RecursiveMode, Watcher};
use std::sync::{Arc, Mutex};

const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

pub(super) struct Watch {
    path: PathBuf,
    _watcher: notify::RecommendedWatcher,
    _events: Task<()>,
}

impl Browser {
    pub(super) fn watch_directory(&mut self, id: u64, path: Option<&Path>, cx: &mut Context<Self>) {
        let Some(slot) = self.tab_slot_mut(id) else {
            return;
        };
        if slot.watch.as_ref().map(|watch| watch.path.as_path()) == path {
            return;
        }
        slot.watch = None;
        let Some(path) = path else { return };
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let pending = Arc::new(Mutex::new(Changes::default()));
        let watcher = match subscribe(path, sender, pending.clone()) {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "cannot watch directory");
                return;
            }
        };
        let runtime = self.window.runtime.clone();
        let events = cx.spawn(async move |this, cx| {
            while receiver.recv().await.is_some() {
                if runtime
                    .spawn(async { tokio::time::sleep(DEBOUNCE).await })
                    .await
                    .is_err()
                {
                    break;
                }
                // Keep changes queued during navigation rather than scheduling another
                // full refresh, which can otherwise loop during sustained downloads.
                loop {
                    let Ok(loading) = this.update(cx, |this, _| {
                        this.tab_slot(id).is_some_and(OpenTab::is_loading)
                    }) else {
                        return;
                    };
                    if !loading {
                        break;
                    }
                    if runtime
                        .spawn(async { tokio::time::sleep(DEBOUNCE).await })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                while receiver.try_recv().is_ok() {}
                let changes = std::mem::take(&mut *pending.lock().expect("watch changes lock"));
                let Ok(snapshot) = this.update(cx, |this, _| {
                    let slot = this.tab_slot(id)?;
                    Some((
                        slot.navigation.revision(),
                        slot.navigation.directory()?.clone(),
                    ))
                }) else {
                    break;
                };
                let Some((revision, snapshot)) = snapshot else {
                    continue;
                };
                let work = cx.background_spawn(async move { changes.apply(snapshot) });
                let result = work.await;
                if this
                    .update(cx, |this, cx| match result {
                        Ok(snapshot) => this.apply_watch_snapshot(id, revision, snapshot, cx),
                        Err(error) => tracing::warn!(%error, "cannot reconcile directory changes"),
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        if let Some(slot) = self.tab_slot_mut(id) {
            slot.watch = Some(Watch {
                path: path.to_owned(),
                _watcher: watcher,
                _events: events,
            });
        }
    }

    fn apply_watch_snapshot(
        &mut self,
        id: u64,
        revision: u64,
        snapshot: directory::DirectorySnapshot,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.tab_slot(id) else {
            return;
        };
        if slot
            .navigation
            .directory()
            .is_some_and(|current| current.entries == snapshot.entries)
        {
            return;
        }
        let previous = slot.table_rows.clone();
        let selected = slot.navigation.view().selected.clone();
        let layout = self.columns_layout(&slot.location()).clone();
        let active = id == self.tabs.active_id && self.pages.is_empty();
        let details = self.details_visible();
        let Some(slot) = self.tab_slot_mut(id) else {
            return;
        };
        if !slot.navigation.update_directory(revision, snapshot) {
            return;
        }
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
        if id == self.tabs.active_id {
            // Keep input handlers and later scrolling current even when no pixels change.
            table::FileTable::update_rows(&self.files.table, rows, changed, cx);
        }
        if notify_browser {
            cx.notify();
        }
    }
}

fn subscribe(
    path: &Path,
    sender: tokio::sync::mpsc::Sender<()>,
    pending: Arc<Mutex<Changes>>,
) -> notify::Result<notify::RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        match event {
            Ok(event) if !matches!(event.kind, EventKind::Access(_)) => {
                pending.lock().expect("watch changes lock").push(event);
            }
            Err(error) => {
                tracing::warn!(%error, "directory watch failed; requesting rescan");
                pending.lock().expect("watch changes lock").rescan();
            }
            _ => return,
        }
        let _ = sender.try_send(());
    })?;
    watcher.watch(path, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}
