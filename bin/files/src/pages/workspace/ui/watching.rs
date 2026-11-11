//! Coalesce filesystem events and reconcile entries without restarting navigation.
mod presentation;
use super::*;

const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

pub(super) struct Watch {
    path: PathBuf,
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
        let mut watcher = match directory::Watching::new(path) {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "cannot watch directory");
                return;
            }
        };
        let tasks = self.window.tasks.clone();
        let events = cx.spawn(async move |this, cx| {
            while watcher.changed().await {
                if tasks
                    .spawn_named("Debounce directory changes", async {
                        tokio::time::sleep(DEBOUNCE).await
                    })
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
                    if tasks
                        .spawn_named("Debounce directory changes", async {
                            tokio::time::sleep(DEBOUNCE).await
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                let changes = watcher.take_changes();
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
                let work = tasks.spawn_blocking_named("Reconcile directory changes", move || {
                    changes.apply(snapshot)
                });
                let result = work
                    .await
                    .map_err(directory::Error::from)
                    .and_then(std::convert::identity);
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
