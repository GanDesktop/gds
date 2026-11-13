//! Optimistic place state with record-level persistence and pending-edit rebasing.

#[cfg(test)]
use std::path::Path;

use std::{
    ops::Deref,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};

use super::{ColumnWidths, Location, ViewScope};
use super::{
    api::places::Store,
    model::places::{Change, Places as Snapshot},
};
use crate::entities::file::x::location::{Layout, Sort, ViewMode};

pub(crate) use super::api::places::Error;

/// Application-owned storage; windows share its pool but load independent state.
pub(crate) struct Storage {
    path: PathBuf,
    store: tokio::sync::OnceCell<Arc<Store>>,
    windows: Mutex<Vec<Shutdown>>,
}

impl Storage {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            store: tokio::sync::OnceCell::new(),
            windows: Mutex::default(),
        }
    }

    /// Start one application-owned flush hook before opening any windows.
    pub(crate) fn start(path: PathBuf, tasks: &gds::service::task_manager::Scope) -> Arc<Self> {
        let storage = Arc::new(Self::new(path));
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
        storage
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
            .get_or_try_init(|| async { Store::open_at(tasks, &self.path).await.map(Arc::new) })
            .await?;
        let places = Places {
            snapshot: store.read().await?,
            store: Some(store.clone()),
            pending: Arc::default(),
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
    #[cfg(all(test, feature = "gui-tests"))]
    pub(crate) fn for_tests(pinned: Vec<Location>, recent: Vec<Location>) -> Self {
        Self {
            snapshot: Snapshot {
                pinned,
                recent,
                ..Snapshot::default()
            },
            ..Self::default()
        }
    }

    pub(crate) fn shutdown(&self) -> Option<Shutdown> {
        Some(Shutdown {
            store: Arc::downgrade(self.store.as_ref()?),
            pending: Arc::downgrade(&self.pending),
        })
    }

    pub(crate) fn reload(&self) -> Option<impl Future<Output = Result<Snapshot, Error>> + use<>> {
        let store = self.store.clone()?;
        Some(async move { store.read().await })
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

    pub(crate) fn accept(&mut self, mut snapshot: Snapshot) {
        for change in self.pending.lock().expect("place edits lock").iter() {
            change.apply(&mut snapshot);
        }
        self.snapshot = snapshot;
    }

    pub(crate) fn refresh_favorites(&mut self, fresh: Snapshot) {
        self.snapshot.favorites = fresh.favorites;
        for change in self.pending.lock().expect("place edits lock").iter() {
            if matches!(change, Change::Favorite { .. }) {
                change.apply(&mut self.snapshot);
            }
        }
    }
}

#[cfg(test)]
mod tests;
