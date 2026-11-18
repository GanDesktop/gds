//! Application-owned volume discovery and serialized device operations.
use super::*;
use anti_webui::prelude::*;
use gds::{app::Gds, service::task_manager::Scope};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::mpsc;

pub(super) struct Observation {
    pub started: Instant,
    pub result: Result<Snapshot, Error>,
}

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
    revision: Signal<u64>,
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
            revision: signal(0),
            busy: Arc::new(Mutex::new(HashMap::new())),
            usage: RefCell::new(super::usage::UsageCache::default()),
            tasks: tasks.clone(),
        }));
        let weak = Rc::downgrade(&service.0);
        let updates = ui_channel(move |observation| {
            if let Some(state) = weak.upgrade() {
                Service(state).apply(observation);
            }
        });
        let (sender, mut receiver) = mpsc::channel(16);
        let cancel = tasks.token();
        let watcher = tasks.spawn_named("Watch storage volumes", async move {
            tokio::select! { _ = cancel.cancelled() => {}, _ = super::linux::watch(sender) => {} }
        });
        tasks.spawn_named("Deliver storage volumes", async move {
            while let Some(observation) = receiver.recv().await {
                updates.send(observation);
            }
            if let Err(error) = watcher.await {
                updates.send(Observation {
                    started: Instant::now(),
                    result: Err(Error::Operation(error.to_string())),
                });
            }
        });
        service
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
            Gds::current()
                .tasks()
                .spawn("Refresh storage volumes", super::linux::refresh())
                .then(move |result| {
                    service.apply(Observation {
                        started,
                        result: result
                            .unwrap_or_else(|error| Err(Error::Operation(error.to_string()))),
                    });
                })
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
                Gds::current()
                    .tasks()
                    .spawn("Deliver filesystem capacity", work)
                    .then(move |result| {
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
                    })
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
    /// Rejects disconnected devices and overlapping operations.
    pub fn operate(
        &self,
        action: DeviceAction,
    ) -> Result<Task<Result<OperationOutcome, Error>>, Error> {
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
        Ok(Gds::current().with(|| {
            Gds::current()
                .tasks()
                .spawn("Change volume mount state", async move {
                    let mount_point = super::linux::operate(expected, action).await;
                    let started = Instant::now();
                    let result = super::linux::refresh().await;
                    drop(guard);
                    (mount_point, Observation { started, result })
                })
                .map(move |result| {
                    let (mount_point, observation) =
                        result.map_err(|error| Error::Operation(error.to_string()))?;
                    let refresh_result = observation.result.clone();
                    service.apply(observation);
                    service.changed();
                    let mount_point = mount_point?;
                    refresh_result?;
                    Ok(OperationOutcome { mount_point })
                })
        }))
    }
}
