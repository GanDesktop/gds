//! Filesystem observation and snapshot reconciliation, independent of a window.
mod changes;
pub(crate) use changes::Changes;
pub use changes::{DEFAULT_WATCH_DEBOUNCE_MS, DEFAULT_WATCH_INCREMENTAL_LIMIT};
use notify::{EventKind, RecursiveMode, Watcher};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

pub(crate) struct Watching {
    _watcher: notify::RecommendedWatcher,
    events: mpsc::Receiver<()>,
    pending: Arc<Mutex<Changes>>,
}

impl Watching {
    /// # Errors
    /// Reports failures creating or installing the filesystem watch.
    #[allow(dead_code)]
    pub(crate) fn new(path: &Path) -> notify::Result<Self> {
        Self::with_limit(path, DEFAULT_WATCH_INCREMENTAL_LIMIT)
    }

    /// # Errors
    /// Reports failures creating or installing the filesystem watch.
    pub(crate) fn with_limit(path: &Path, incremental_limit: usize) -> notify::Result<Self> {
        let (sender, events) = mpsc::channel(1);
        let pending = Arc::new(Mutex::new(Changes::with_limit(incremental_limit)));
        let watcher = Self::subscribe(path, sender, pending.clone())?;
        Ok(Self {
            _watcher: watcher,
            events,
            pending,
        })
    }

    pub(crate) async fn changed(&mut self) -> bool {
        self.events.recv().await.is_some()
    }

    pub(crate) fn take_changes(&mut self) -> Changes {
        while self.events.try_recv().is_ok() {}
        let mut pending = self.pending.lock().expect("watch changes lock");
        let limit = pending.max_paths;
        std::mem::replace(&mut *pending, Changes::with_limit(limit))
    }

    fn subscribe(
        path: &Path,
        sender: mpsc::Sender<()>,
        pending: Arc<Mutex<Changes>>,
    ) -> notify::Result<notify::RecommendedWatcher> {
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
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
}
