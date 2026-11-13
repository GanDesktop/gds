//! Filesystem observation and snapshot reconciliation, independent of a window.
mod changes;
pub(crate) use changes::Changes;
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
    pub(crate) fn new(path: &Path) -> notify::Result<Self> {
        let (sender, events) = mpsc::channel(1);
        let pending = Arc::new(Mutex::new(Changes::default()));
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
        std::mem::take(&mut *self.pending.lock().expect("watch changes lock"))
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
