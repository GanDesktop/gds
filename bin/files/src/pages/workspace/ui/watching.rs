//! Each local tab owns its watch and debounced event consumer.
use super::*;
use notify::{EventKind, RecursiveMode, Watcher};

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
        let result = subscribe(path, sender);
        let watcher = match result {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "cannot watch directory");
                return;
            }
        };
        let runtime = self.window.runtime.clone();
        let events = cx.spawn(async move |this, cx| {
            while receiver.recv().await.is_some() {
                let delay = runtime.spawn(async { tokio::time::sleep(DEBOUNCE).await });
                if delay.await.is_err() {
                    break;
                }
                while receiver.try_recv().is_ok() {}
                if this
                    .update(cx, |this, cx| {
                        let Some(slot) = this.tab_slot_mut(id) else {
                            return;
                        };
                        if slot.is_loading() {
                            slot.needs_refresh = true;
                        } else {
                            this.refresh_tab(id, cx);
                        }
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
}

fn subscribe(
    path: &Path,
    sender: tokio::sync::mpsc::Sender<()>,
) -> notify::Result<notify::RecommendedWatcher> {
    let watcher =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
            Ok(event)
                if matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Modify(_)
                        | EventKind::Remove(_)
                        | EventKind::Any
                        | EventKind::Other
                ) =>
            {
                let _ = sender.try_send(());
            }
            Err(error) => tracing::warn!(%error, "directory watch failed"),
            _ => {}
        });
    watcher.and_then(|mut watcher| {
        watcher.watch(path, RecursiveMode::NonRecursive)?;
        Ok(watcher)
    })
}

#[cfg(test)]
mod tests;
