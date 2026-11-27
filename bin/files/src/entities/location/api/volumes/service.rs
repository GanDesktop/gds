//! Application-owned volume discovery and serialized device operations.

use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};

use gds::{app::Gds, service::task_manager::Scope};
use tokio::sync::mpsc;
use zgui::prelude::*;

use super::*;
use super::{Error, Observation};

struct BusyGuard {
    busy: Arc<Mutex<HashMap<VolumeId, Vec<PathBuf>>>>,
    scope: Vec<VolumeId>,
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        let mut busy = self.busy.lock().expect("volumes busy lock");
        for id in &self.scope {
            busy.remove(id);
        }
    }
}

#[derive(Clone)]
pub struct Service(Rc<State>);

struct State {
    observation: RefCell<Observed>,
    revision: RwSignal<u64, LocalStorage>,
    busy: Arc<Mutex<HashMap<VolumeId, Vec<PathBuf>>>>,
    usage: RefCell<super::usage::UsageCache>,
    tasks: Scope,
}

struct Observed {
    discovery: Discovery,
    started: Instant,
}

enum Discovery {
    Refreshing(Snapshot),
    Ready(Snapshot),
    Failed { snapshot: Snapshot, error: String },
}

impl Discovery {
    fn snapshot(&self) -> &Snapshot {
        match self {
            Self::Refreshing(snapshot) | Self::Ready(snapshot) | Self::Failed { snapshot, .. } => {
                snapshot
            }
        }
    }
}

impl Service {
    pub fn new(tasks: Scope) -> Self {
        let service = Self(Rc::new(State {
            observation: RefCell::new(Observed {
                discovery: Discovery::Refreshing(Snapshot::default()),
                started: Instant::now(),
            }),
            revision: RwSignal::new_local(0),
            busy: Arc::new(Mutex::new(HashMap::new())),
            usage: RefCell::new(super::usage::UsageCache::default()),
            tasks: tasks.clone(),
        }));
        let (sender, mut receiver) = mpsc::channel(16);
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        let watcher = {
            let cancel = tasks.token();
            tasks.spawn_named("Watch storage volumes", async move {
                tokio::select! {
                    _ = cancel.cancelled() => {},
                    _ = super::udisks::watch(sender) => {},
                }
            })
        };
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        let watcher = tasks.spawn_named("Discover storage volumes", async move {
            let _ = sender
                .send(Observation {
                    started: Instant::now(),
                    result: Err(Error::Unsupported),
                })
                .await;
        });
        let weak = Rc::downgrade(&service.0);
        Gds::current().with(|| {
            zgui::task::spawn_local(async move {
                while let Some(observation) = receiver.recv().await {
                    let Some(state) = weak.upgrade() else {
                        return;
                    };
                    Service(state).apply(observation);
                }
                if let Err(error) = watcher.await
                    && let Some(state) = weak.upgrade()
                {
                    Service(state).apply(Observation {
                        started: Instant::now(),
                        result: Err(Error::Operation(error.to_string())),
                    });
                }
            });
        });
        service
    }

    #[cfg(test)]
    pub fn with_snapshot(tasks: Scope, snapshot: Snapshot) -> Self {
        Self(Rc::new(State {
            observation: RefCell::new(Observed {
                discovery: Discovery::Ready(snapshot),
                started: Instant::now(),
            }),
            revision: RwSignal::new_local(0),
            busy: Arc::new(Mutex::new(HashMap::new())),
            usage: RefCell::new(super::usage::UsageCache::default()),
            tasks,
        }))
    }

    fn changed(&self) {
        self.0
            .revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    fn track(&self) {
        self.0.revision.get();
    }

    pub fn snapshot(&self) -> Snapshot {
        self.track();
        self.0.observation.borrow().discovery.snapshot().clone()
    }

    pub fn snapshot_untracked(&self) -> Snapshot {
        self.0.observation.borrow().discovery.snapshot().clone()
    }

    pub fn error(&self) -> Option<String> {
        self.track();
        match &self.0.observation.borrow().discovery {
            Discovery::Failed { error, .. } => Some(error.clone()),
            _ => None,
        }
    }

    pub fn discovering(&self) -> bool {
        self.track();
        matches!(
            self.0.observation.borrow().discovery,
            Discovery::Refreshing(_)
        )
    }

    pub fn is_busy(&self, id: &VolumeId) -> bool {
        self.track();
        self.is_busy_untracked(id)
    }

    pub fn is_busy_untracked(&self, id: &VolumeId) -> bool {
        self.0
            .busy
            .lock()
            .expect("volumes busy lock")
            .contains_key(id)
    }

    pub fn capacity(&self, id: &VolumeId) -> Option<CapacityState> {
        self.track();
        self.0.usage.borrow().get(id).cloned()
    }

    pub fn removal_in_progress(&self, path: &Path) -> bool {
        self.0
            .busy
            .lock()
            .expect("volumes busy lock")
            .values()
            .flatten()
            .any(|root| path.starts_with(root))
    }

    pub fn refresh(&self) {
        {
            let mut state = self.0.observation.borrow_mut();
            state.discovery = Discovery::Refreshing(state.discovery.snapshot().clone());
        }
        self.changed();
        let started = Instant::now();
        let service = self.clone();
        Gds::current().with(|| {
            Gds::current().tasks().spawn_ui(
                "Refresh storage volumes",
                async {
                    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                    {
                        super::udisks::refresh().await
                    }
                    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
                    {
                        Err(Error::Unsupported)
                    }
                },
                move |result| {
                    service.apply(Observation {
                        started,
                        result: result
                            .unwrap_or_else(|error| Err(Error::Operation(error.to_string()))),
                    });
                },
            );
        });
        self.refresh_capacity();
    }

    pub fn refresh_capacity(&self) {
        self.0.usage.borrow_mut().invalidate();
        self.sample_capacity();
    }

    fn sample_capacity(&self) {
        let requests = self.0.usage.borrow_mut().requests(
            self.0.observation.borrow().discovery.snapshot(),
            Instant::now(),
        );
        for request in requests {
            let worker = request.clone();
            let work = self
                .0
                .tasks
                .spawn_blocking_named("Read filesystem capacity", move || worker.read());
            let service = self.clone();
            Gds::current().with(|| {
                Gds::current().tasks().spawn_ui(
                    "Deliver filesystem capacity",
                    work,
                    move |result| {
                        let result = result
                            .and_then(std::convert::identity)
                            .map_err(|error| {
                                super::usage::ReadError::Io(std::io::Error::other(error))
                            })
                            .and_then(std::convert::identity);
                        let changed =
                            service
                                .0
                                .usage
                                .borrow_mut()
                                .finish(request, result, Instant::now());
                        if changed {
                            service.changed();
                        }
                    },
                );
            });
        }
    }

    fn apply(&self, observation: Observation) {
        {
            let mut state = self.0.observation.borrow_mut();
            if observation.started < state.started {
                return;
            }
            state.started = observation.started;
            state.discovery = match observation.result {
                Ok(snapshot) => Discovery::Ready(snapshot),
                Err(error) => Discovery::Failed {
                    snapshot: state.discovery.snapshot().clone(),
                    error: error.to_string(),
                },
            };
        }
        self.changed();
        self.sample_capacity();
    }

    /// # Errors
    /// Rejects disconnected devices and overlapping operations. Backend errors
    /// preserve their system explanation, including busy/authorization failures.
    pub fn operate(
        &self,
        action: DeviceAction,
    ) -> Result<impl Future<Output = Result<(), Error>> + 'static, Error> {
        let expected = self.0.observation.borrow().discovery.snapshot().clone();
        if expected.volume(action.volume()).is_none() {
            return Err(Error::Disconnected);
        }
        if action.removes() && !expected.supports_removal(&action) {
            return Err(Error::Unsupported);
        }
        let mut scope: Vec<_> = expected
            .removal_volumes(&action)
            .iter()
            .map(|volume| volume.id.clone())
            .collect();
        if scope.is_empty() {
            scope.push(action.volume().clone());
        }
        {
            let mut busy = self.0.busy.lock().expect("volumes busy lock");
            if scope.iter().any(|id| busy.contains_key(id)) {
                return Err(Error::Busy);
            }
            for id in &scope {
                busy.insert(
                    id.clone(),
                    expected
                        .volume(id)
                        .map(|volume| volume.mount_points.clone())
                        .unwrap_or_default(),
                );
            }
        }
        let guard = BusyGuard {
            busy: self.0.busy.clone(),
            scope,
        };
        self.changed();
        let service = self.clone();
        let work = Gds::current().with(|| {
            Gds::current()
                .tasks()
                .spawn("Change volume mount state", async move {
                    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                    {
                        let mount_point = super::udisks::operate(expected, action).await;
                        let started = Instant::now();
                        let result = super::udisks::refresh().await;
                        (mount_point, Observation { started, result })
                    }
                    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
                    {
                        let _ = (expected, action);
                        (
                            Err(Error::Unsupported),
                            Observation {
                                started: Instant::now(),
                                result: Err(Error::Unsupported),
                            },
                        )
                    }
                })
        });
        Ok(async move {
            let guard = guard;
            let (mount_point, observation) = work
                .await
                .map_err(|error| Error::Operation(error.to_string()))?;
            drop(guard);
            let refresh_result = observation.result.clone();
            service.apply(observation);
            service.changed();
            let _ = mount_point?;
            refresh_result?;
            Ok(())
        })
    }
}
