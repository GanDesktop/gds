//! Optimistic place state with record-level persistence and pending-edit rebasing.

use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use tokio::sync::{mpsc, watch};

use super::{ColumnWidths, Location, ViewScope};
use super::{
    api::places::{Store, UserDirsSource, XdgUserDirs},
    model::places::{Change, Places as Snapshot},
};
use crate::entities::file::x::location::{Layout, Sort, ViewMode};

pub(crate) use super::api::places::Error;

const USER_DIRS_WATCH_DEBOUNCE: Duration = Duration::from_millis(250);

/// Application-owned storage; windows share its pool but load independent state.
pub(crate) struct Storage {
    path: PathBuf,
    user_dirs_source: Option<UserDirsSource>,
    store: tokio::sync::OnceCell<Arc<Store>>,
    windows: Mutex<Vec<Shutdown>>,
    updates: watch::Sender<Option<Snapshot>>,
}

impl Storage {
    #[cfg(test)]
    pub(crate) fn new(path: PathBuf) -> Self {
        Self::new_with_user_dirs(path, UserDirsSource::current())
    }

    fn new_with_user_dirs(path: PathBuf, user_dirs_source: Option<UserDirsSource>) -> Self {
        let (updates, _) = watch::channel(None);
        Self {
            path,
            user_dirs_source,
            store: tokio::sync::OnceCell::new(),
            windows: Mutex::default(),
            updates,
        }
    }

    /// Start one application-owned flush hook and live `user-dirs.dirs` watcher
    /// before opening any windows.
    pub(crate) fn start(path: PathBuf, tasks: &gds::service::task_manager::Scope) -> Arc<Self> {
        Self::start_with_user_dirs(path, UserDirsSource::current(), tasks)
    }

    pub(crate) fn start_with_user_dirs(
        path: PathBuf,
        user_dirs_source: Option<UserDirsSource>,
        tasks: &gds::service::task_manager::Scope,
    ) -> Arc<Self> {
        let storage = Arc::new(Self::new_with_user_dirs(path, user_dirs_source));
        let weak = Arc::downgrade(&storage);
        let cancel = tasks.token();
        tasks.spawn_named("Flush saved locations on exit", async move {
            cancel.cancelled().await;
            let Some(storage) = weak.upgrade() else {
                return;
            };
            let windows =
                std::mem::take(&mut *storage.windows.lock().expect("places windows lock"));
            for shutdown in windows {
                if let Err(error) = shutdown.run().await {
                    tracing::error!(%error, "Cannot save Files state during shutdown");
                }
            }
        });
        if let Some(source) = storage.user_dirs_source.clone() {
            Self::spawn_user_dirs_watcher(&storage, source, tasks);
        }
        storage
    }

    fn spawn_user_dirs_watcher(
        storage: &Arc<Self>,
        source: UserDirsSource,
        tasks: &gds::service::task_manager::Scope,
    ) {
        let (raw_tx, mut raw_rx) = mpsc::channel(4);
        let mut sources = gds::service::desktop::watch::Sources::new(raw_tx);
        if let Err(error) = sources.set_paths(vec![source.config_path.clone()]) {
            tracing::warn!(
                path = %source.config_path.display(),
                %error,
                "cannot watch XDG user-dirs.dirs"
            );
            return;
        }
        let weak = Arc::downgrade(storage);
        let cancel = tasks.token();
        drop(tasks.spawn_named("Watch XDG user-dirs.dirs", async move {
            enum WatchEvent {
                Cancelled,
                Changed,
                Closed,
            }
            enum DebounceEvent {
                Cancelled,
                Ready,
                Changed,
                Closed,
            }

            loop {
                let event = tokio::select! {
                    () = cancel.cancelled() => WatchEvent::Cancelled,
                    received = raw_rx.recv() => match received {
                        Some(()) => WatchEvent::Changed,
                        None => WatchEvent::Closed,
                    },
                };
                match event {
                    WatchEvent::Cancelled | WatchEvent::Closed => return,
                    WatchEvent::Changed => {}
                }

                let deadline = tokio::time::Instant::now() + USER_DIRS_WATCH_DEBOUNCE;
                loop {
                    let event = tokio::select! {
                        () = cancel.cancelled() => DebounceEvent::Cancelled,
                        () = tokio::time::sleep_until(deadline) => DebounceEvent::Ready,
                        received = raw_rx.recv() => match received {
                            Some(()) => DebounceEvent::Changed,
                            None => DebounceEvent::Closed,
                        },
                    };
                    match event {
                        DebounceEvent::Cancelled | DebounceEvent::Closed => return,
                        DebounceEvent::Ready => break,
                        DebounceEvent::Changed => {}
                    }
                }

                if let Err(error) = sources.refresh() {
                    tracing::warn!(
                        path = %source.config_path.display(),
                        %error,
                        "cannot refresh XDG user-dirs.dirs watch"
                    );
                }

                let Some(storage) = weak.upgrade() else {
                    return;
                };
                if storage.store.get().is_none() {
                    continue;
                }
                let load_source = source.clone();
                let loaded = tokio::task::spawn_blocking(move || load_source.load()).await;
                let xdg_dirs = match loaded {
                    Ok(Ok(dirs)) => dirs,
                    Ok(Err(error)) => {
                        tracing::warn!(
                            path = %source.config_path.display(),
                            %error,
                            "cannot reload XDG user-dirs.dirs"
                        );
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "XDG user-dirs.dirs reload task failed");
                        continue;
                    }
                };
                if let Err(error) = storage
                    .sync_xdg_user_dirs(Some(source.home.as_path()), &xdg_dirs)
                    .await
                {
                    tracing::warn!(%error, "cannot sync Quick Access with XDG user-dirs.dirs");
                }
            }
        }));
    }

    /// Subscribe to external store updates (such as live `user-dirs.dirs` sync).
    #[cfg(test)]
    pub(crate) fn subscribe_updates(&self) -> watch::Receiver<Option<Snapshot>> {
        self.updates.subscribe()
    }

    /// Reconcile an open store against `xdg_dirs`, publishing the updated
    /// snapshot to all subscribers when Quick Access changes.
    ///
    /// # Errors
    /// Reports database or serialization failures while syncing.
    pub(crate) async fn sync_xdg_user_dirs(
        &self,
        home: Option<&Path>,
        xdg_dirs: &XdgUserDirs,
    ) -> Result<bool, Error> {
        let Some(store) = self.store.get() else {
            return Ok(false);
        };
        let outcome = store.sync_xdg_user_dirs(home, xdg_dirs).await?;
        if outcome.changed {
            let _ = self.updates.send_replace(Some(outcome.places));
        }
        Ok(outcome.changed)
    }

    /// Load a window's snapshot, initializing the shared pool once on success.
    ///
    /// # Errors
    /// Reports storage or invalid-record errors. Failed initialization can be retried.
    pub(crate) async fn load(
        &self,
        tasks: &gds::service::task_manager::Scope,
    ) -> Result<Places, Error> {
        let store = self
            .store
            .get_or_try_init(|| async {
                match &self.user_dirs_source {
                    Some(source) => {
                        let load_source = source.clone();
                        let xdg_dirs =
                            match tokio::task::spawn_blocking(move || load_source.load()).await {
                                Ok(Ok(dirs)) => dirs,
                                Ok(Err(error)) => {
                                    tracing::warn!(
                                        path = %source.config_path.display(),
                                        %error,
                                        "cannot read XDG user-dirs.dirs; falling back to default names"
                                    );
                                    XdgUserDirs::resolve(&source.home, None)
                                }
                                Err(error) => {
                                    tracing::warn!(
                                        %error,
                                        "XDG user-dirs.dirs load task failed; falling back to default names"
                                    );
                                    XdgUserDirs::resolve(&source.home, None)
                                }
                            };
                        Store::open_at_with_xdg_dirs(
                            tasks,
                            &self.path,
                            Some(source.home.as_path()),
                            &xdg_dirs,
                        )
                        .await
                        .map(Arc::new)
                    }
                    None => Store::open_at(tasks, &self.path).await.map(Arc::new),
                }
            })
            .await?;
        // Subscribe before reading the snapshot so an update published during
        // `store.read()` is never missed by the new window.
        let updates = self.updates.subscribe();
        let places = Places {
            snapshot: store.read().await?,
            store: Some(store.clone()),
            pending: Arc::default(),
            updates: Some(updates),
        };
        let mut windows = self.windows.lock().expect("places windows lock");
        windows.retain(|window| window.pending.strong_count() > 0);
        windows.push(places.shutdown().expect("loaded places have storage"));
        Ok(places)
    }
}

#[derive(Default)]
pub(crate) struct Places {
    snapshot: Snapshot,
    store: Option<Arc<Store>>,
    pending: Arc<Mutex<Vec<Change>>>,
    updates: Option<watch::Receiver<Option<Snapshot>>>,
}

impl Deref for Places {
    type Target = Snapshot;
    fn deref(&self) -> &Snapshot {
        &self.snapshot
    }
}

pub(crate) struct Save {
    store: Arc<Store>,
    pending: Arc<Mutex<Vec<Change>>>,
}

/// Weak handles let the app await queued edits on exit without retaining
/// pending queues for windows that have already closed.
#[derive(Clone)]
pub(crate) struct Shutdown {
    store: Weak<Store>,
    pending: Weak<Mutex<Vec<Change>>>,
}

impl Shutdown {
    /// # Errors
    /// Reports failures while flushing the remaining place edits.
    pub(crate) async fn run(self) -> Result<(), Error> {
        if let Some(store) = self.store.upgrade()
            && let Some(pending) = self.pending.upgrade()
        {
            Save { store, pending }.run().await?;
        }
        Ok(())
    }
}

impl Save {
    /// # Errors
    /// Reports storage failures and retains the failed batch for a later retry.
    pub(crate) async fn run(&self) -> Result<Snapshot, Error> {
        // Keep draining while the UI is awaiting SQLite, even if its window
        // closes. The save owns the queue until every accepted edit is written.
        loop {
            let changes = std::mem::take(&mut *self.pending.lock().expect("place edits lock"));
            match self.store.save(changes.clone()).await {
                Ok(snapshot) if self.pending.lock().expect("place edits lock").is_empty() => {
                    return Ok(snapshot);
                }
                Ok(_) => continue,
                Err(error) => {
                    let mut pending = self.pending.lock().expect("place edits lock");
                    let mut failed = changes;
                    failed.append(&mut pending);
                    *pending = failed;
                    return Err(error);
                }
            }
        }
    }
}

impl Places {
    /// Keep accepted edits alive until the closing window has drained them.
    pub(crate) fn drain_on_close(
        &self,
        tasks: &gds::service::task_manager::Scope,
        closing: tokio_util::sync::CancellationToken,
    ) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let save = Save {
            store,
            pending: self.pending.clone(),
        };
        tasks.spawn_named("Flush closing Files window", async move {
            closing.cancelled().await;
            if let Err(error) = save.run().await {
                tracing::error!(%error, "Cannot flush Files places");
            }
        });
    }

    pub(crate) fn shutdown(&self) -> Option<Shutdown> {
        Some(Shutdown {
            store: Arc::downgrade(self.store.as_ref()?),
            pending: Arc::downgrade(&self.pending),
        })
    }

    fn change(&mut self, change: Change) {
        change.apply(&mut self.snapshot);
        self.pending.lock().expect("place edits lock").push(change);
    }

    pub(crate) fn record(&mut self, location: impl Into<Location>) {
        self.change(Change::Recent(location.into()));
    }
    pub(crate) fn toggle_remote_favorite(&mut self, address: gds::service::remote::Address) {
        self.toggle_favorite(Location::Remote(address));
    }
    pub(crate) fn set_remote_favorite(
        &mut self,
        address: gds::service::remote::Address,
        present: bool,
    ) {
        self.set_favorite(Location::Remote(address), present);
    }
    pub(crate) fn clear_recent(&mut self) {
        self.change(Change::ClearRecent);
    }
    pub(crate) fn toggle_favorite(&mut self, location: impl Into<Location>) {
        let location = location.into();
        let present = !self.is_favorite(&location);
        self.set_favorite(location, present);
    }
    pub(crate) fn set_favorite(&mut self, location: Location, present: bool) {
        self.change(Change::Favorite { location, present });
    }
    pub(crate) fn reorder_pinned(&mut self, locations: Vec<Location>) {
        self.change(Change::PinOrder(locations));
    }
    pub(crate) fn set_pinned(&mut self, location: &Location, present: bool) {
        self.change(Change::Pin {
            location: location.clone(),
            present,
        });
    }
    pub(crate) fn add_quick_access(&mut self, location: impl Into<Location>) {
        self.set_quick_access(location.into(), true);
    }
    pub(crate) fn remove_quick_access(&mut self, location: impl Into<Location>) {
        self.set_quick_access(location.into(), false);
    }
    pub(crate) fn set_quick_access(&mut self, location: Location, present: bool) {
        self.change(Change::QuickAccess { location, present });
    }
    pub(crate) fn reorder_quick_access(&mut self, locations: Vec<Location>) {
        self.change(Change::QuickAccessOrder(locations));
    }
    pub(crate) fn set_columns(&mut self, location: impl Into<Location>, layout: Layout) {
        self.change(Change::Columns {
            scope: ViewScope::Location(location.into()),
            layout: Some(layout),
        });
    }
    pub(crate) fn set_default_columns(&mut self, layout: Layout) {
        self.change(Change::Columns {
            scope: ViewScope::Defaults,
            layout: Some(layout),
        });
    }
    pub(crate) fn clear_columns(&mut self, location: impl Into<Location>) {
        self.change(Change::Columns {
            scope: ViewScope::Location(location.into()),
            layout: None,
        });
    }
    pub(crate) fn set_view_mode(&mut self, location: impl Into<Location>, mode: ViewMode) {
        self.change(Change::ViewMode {
            location: location.into(),
            mode,
        });
    }
    pub(crate) fn set_widths(&mut self, location: Location, widths: ColumnWidths) {
        self.change(Change::Widths { location, widths });
    }
    pub(crate) fn set_sort(&mut self, location: Location, sort: Sort) {
        self.change(Change::Sort { location, sort });
    }
    pub(crate) fn set_details(&mut self, location: Location, visible: bool) {
        self.change(Change::Details { location, visible });
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.lock().expect("place edits lock").is_empty()
    }

    pub(crate) fn take_save(&mut self) -> Option<Save> {
        if !self.has_pending() {
            return None;
        }
        Some(Save {
            store: self.store.clone()?,
            pending: self.pending.clone(),
        })
    }

    pub(crate) fn subscribe_external_updates(&self) -> Option<watch::Receiver<Option<Snapshot>>> {
        self.updates.clone()
    }

    pub(crate) fn accept(&mut self, mut snapshot: Snapshot) {
        for change in self.pending.lock().expect("place edits lock").iter() {
            change.apply(&mut snapshot);
        }
        self.snapshot = snapshot;
    }
}

#[cfg(test)]
mod tests;
