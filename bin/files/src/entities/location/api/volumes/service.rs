use gds::service::task_manager::Scope;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use gds::service::task_manager::Task as IoTask;
use gpui_kit::*;
use tokio::sync::mpsc;

use super::*;

struct SharedService(Entity<Service>);
impl Global for SharedService {}

pub(super) struct Observation {
    pub started: Instant,
    pub result: Result<Snapshot, Error>,
}

struct ActionReport {
    mount_point: Result<Option<std::path::PathBuf>, Error>,
    observation: Observation,
}

struct BusyGuard {
    busy: Arc<Mutex<HashMap<VolumeId, Vec<std::path::PathBuf>>>>,
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

/// One service per application, shared by every window and tab.
pub struct Service {
    pub snapshot: Snapshot,
    pub error: Option<String>,
    pub discovering: bool,
    busy: Arc<Mutex<HashMap<VolumeId, Vec<std::path::PathBuf>>>>,
    last_observation: Instant,
    usage: super::usage::UsageCache,
    tasks: Scope,
    _updates: Task<()>,
}

impl Service {
    pub fn initialize(tasks: Scope, cx: &mut App) -> Entity<Self> {
        assert!(
            !cx.has_global::<SharedService>(),
            "volumes already initialized"
        );
        let service = cx.new(|cx| Self::new(tasks, cx));
        cx.set_global(SharedService(service.clone()));
        service
    }

    pub fn get(cx: &App) -> Entity<Self> {
        cx.global::<SharedService>().0.clone()
    }

    fn new(tasks: Scope, cx: &mut Context<Self>) -> Self {
        let last_observation = Instant::now();
        let (sender, mut receiver) = mpsc::channel(16);
        #[cfg(target_os = "linux")]
        let watcher = {
            let cancel = tasks.token();
            tasks.spawn_named("Watch storage volumes", async move {
                tokio::select! { _ = cancel.cancelled() => {}, _ = super::linux::watch(sender) => {} }
            })
        };
        #[cfg(not(target_os = "linux"))]
        let watcher = tasks.spawn_named("Discover storage volumes", async move {
            let _ = sender
                .send(Observation {
                    started: Instant::now(),
                    result: Err(Error::Unsupported),
                })
                .await;
        });
        let watcher = IoTask::cancel_on_drop(watcher);
        let updates = cx.spawn(async move |this, cx| {
            while let Some(observation) = receiver.recv().await {
                if this
                    .update(cx, |service, cx| {
                        service.apply(observation, cx);
                    })
                    .is_err()
                {
                    return;
                }
            }
            // The watcher owns the sender; a closed channel also lets us
            // observe its task result without another forwarding task.
            if let Err(error) = watcher.await {
                let _ = this.update(cx, |service, cx| {
                    service.apply(
                        Observation {
                            started: Instant::now(),
                            result: Err(Error::Operation(error.to_string())),
                        },
                        cx,
                    );
                });
            }
        });
        Self {
            snapshot: Snapshot::default(),
            error: None,
            discovering: true,
            busy: Arc::new(Mutex::new(HashMap::new())),
            last_observation,
            usage: super::usage::UsageCache::default(),
            tasks,
            _updates: updates,
        }
    }

    pub fn is_busy(&self, id: &VolumeId) -> bool {
        self.busy
            .lock()
            .expect("volumes busy lock")
            .contains_key(id)
    }

    pub fn capacity(&self, id: &VolumeId) -> Option<&CapacityState> {
        self.usage.get(id)
    }

    pub fn refresh_capacity(&mut self, cx: &mut Context<Self>) {
        self.usage.invalidate();
        self.sample_capacity(cx);
    }

    /// Re-read volume metadata as well as space usage after an explicit refresh.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.discovering = true;
        let started = Instant::now();
        let work = IoTask::cancel_on_drop(self.tasks.spawn_cancellable_named(
            "Refresh storage volumes",
            async {
                #[cfg(target_os = "linux")]
                {
                    super::linux::refresh().await
                }
                #[cfg(not(target_os = "linux"))]
                {
                    Err(Error::Unsupported)
                }
            },
        ));
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .unwrap_or_else(|error| Err(Error::Operation(error.to_string())));
            let _ = this.update(cx, |service, cx| {
                service.apply(Observation { started, result }, cx);
            });
        })
        .detach();
        self.refresh_capacity(cx);
        cx.notify();
    }

    fn sample_capacity(&mut self, cx: &mut Context<Self>) {
        for request in self.usage.requests(&self.snapshot, Instant::now()) {
            let worker_request = request.clone();
            let work = self
                .tasks
                .spawn_blocking_named("Read filesystem capacity", move || worker_request.read());
            cx.spawn(async move |this, cx| {
                let result = work
                    .await
                    .map_err(|error| super::usage::ReadError::Io(std::io::Error::other(error)))
                    .and_then(std::convert::identity);
                let _ = this.update(cx, |service, cx| {
                    if service.usage.finish(request, result, Instant::now()) {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    pub fn removal_in_progress(&self, path: &std::path::Path) -> bool {
        self.busy
            .lock()
            .expect("volumes busy lock")
            .values()
            .flatten()
            .any(|root| path.starts_with(root))
    }

    fn apply(&mut self, observation: Observation, cx: &mut Context<Self>) {
        if observation.started < self.last_observation {
            return;
        }
        self.last_observation = observation.started;
        let previous_snapshot = self.snapshot.clone();
        let previous_error = self.error.clone();
        let was_discovering = self.discovering;
        self.discovering = false;
        match observation.result {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error.to_string());
            }
        }
        if was_discovering || previous_snapshot != self.snapshot || previous_error != self.error {
            cx.notify();
        }
        self.sample_capacity(cx);
    }

    /// # Errors
    /// Rejects disconnected devices and overlapping operations. Backend errors
    /// preserve their system explanation, including busy/authorization failures.
    pub fn operate(
        &mut self,
        action: DeviceAction,
        cx: &mut Context<Self>,
    ) -> Result<Task<Result<OperationOutcome, Error>>, Error> {
        if self.snapshot.volume(action.volume()).is_none() {
            return Err(Error::Disconnected);
        }
        if action.removes() && !self.snapshot.supports_removal(&action) {
            return Err(Error::Unsupported);
        }
        let mut scope: Vec<_> = self
            .snapshot
            .removal_volumes(&action)
            .iter()
            .map(|volume| volume.id.clone())
            .collect();
        if scope.is_empty() {
            scope.push(action.volume().clone());
        }
        {
            let mut busy = self.busy.lock().expect("volumes busy lock");
            if scope.iter().any(|id| busy.contains_key(id)) {
                return Err(Error::Busy);
            }
            for id in &scope {
                let roots = self
                    .snapshot
                    .volume(id)
                    .map(|volume| volume.mount_points.clone())
                    .unwrap_or_default();
                busy.insert(id.clone(), roots);
            }
        }
        let guard = BusyGuard {
            busy: self.busy.clone(),
            scope,
        };
        cx.notify();
        let expected = self.snapshot.clone();
        let work = IoTask::cancel_on_drop(self.tasks.spawn_named(
            "Change volume mount state",
            async move {
                #[cfg(target_os = "linux")]
                {
                    let mount_point = super::linux::operate(expected, action).await;
                    let started = Instant::now();
                    let result = super::linux::refresh().await;
                    ActionReport {
                        mount_point,
                        observation: Observation { started, result },
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = (expected, action);
                    ActionReport {
                        mount_point: Err(Error::Unsupported),
                        observation: Observation {
                            started: Instant::now(),
                            result: Err(Error::Unsupported),
                        },
                    }
                }
            },
        ));
        Ok(cx.spawn(async move |this, cx| {
            let guard = guard;
            let report = work.await.unwrap_or_else(|error| {
                let error = Error::Operation(error.to_string());
                ActionReport {
                    mount_point: Err(error.clone()),
                    observation: Observation {
                        started: Instant::now(),
                        result: Err(error),
                    },
                }
            });
            drop(guard);
            let refresh_result = report.observation.result.clone();
            let _ = this.update(cx, |service, cx| {
                service.apply(report.observation, cx);
                cx.notify();
            });
            let mount_point = report.mount_point?;
            refresh_result?;
            Ok(OperationOutcome { mount_point })
        }))
    }
}
