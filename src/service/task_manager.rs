//! Supervised execution, cooperative cancellation and completion tracking.
use std::{
    fmt::Display,
    future::Future,
    sync::{Arc, Mutex},
};
use tokio_util::{
    sync::CancellationToken,
    task::{TaskTracker, task_tracker::TaskTrackerToken},
};

#[derive(Debug, thiserror::Error)]
#[error("task scope is shutting down")]
pub struct Closed;

struct Node {
    inspection: Option<inspection::ScopeInfo>,
    cancel: CancellationToken,
    tasks: TaskTracker,
}

/// A task belongs to this scope and every ancestor. Executors do not own policy.
#[derive(Clone)]
pub struct Scope {
    admission: Arc<Mutex<Option<inspection::Registry>>>,
    path: Vec<Arc<Node>>,
    runtime: Option<tokio::runtime::Handle>,
}

impl Default for Scope {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Scope {
    /// Bootstrap an executor-bound root without exposing the handle to services.
    pub fn with_handle(runtime: tokio::runtime::Handle, inspect: bool) -> Self {
        Self {
            runtime: Some(runtime),
            ..Self::new(inspect)
        }
    }

    /// Bind the application's executor once. Services receive derived scopes only.
    pub fn with_runtime(runtime: &tokio::runtime::Runtime, inspect: bool) -> Self {
        Self {
            runtime: Some(runtime.handle().clone()),
            ..Self::new(inspect)
        }
    }

    /// Choose diagnostic collection before starting services. Completion tracking
    /// and cancellation remain enabled independently of this option.
    pub fn new(inspect: bool) -> Self {
        let node = Arc::new(Node {
            inspection: inspect.then(|| inspection::ScopeInfo {
                id: 1,
                parent: None,
                name: "Application".into(),
            }),
            cancel: CancellationToken::new(),
            tasks: TaskTracker::new(),
        });
        let registry = inspect.then(|| {
            let mut registry = inspection::Registry {
                next: 1,
                ..Default::default()
            };
            registry.scopes.insert(1, Arc::downgrade(&node));
            registry
        });
        Self {
            admission: Arc::new(Mutex::new(registry)),
            path: vec![node],
            runtime: None,
        }
    }
}

/// Owned by a service or view, never by its worker. Dropping it requests shutdown.
pub struct Owner(Scope);
impl std::ops::Deref for Owner {
    type Target = Scope;
    fn deref(&self) -> &Scope {
        &self.0
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Register before scheduling; release only after resource cleanup finishes.
pub struct Completion {
    tokens: Vec<TaskTrackerToken>,
    inspection: Option<inspection::Registration>,
}

impl Scope {
    fn node(&self) -> &Node {
        self.path.last().unwrap()
    }

    pub fn inspection_enabled(&self) -> bool {
        self.node().inspection.is_some()
    }

    pub fn child(&self) -> Owner {
        self.named_child("Scope")
    }

    pub fn named_child(&self, name: impl Display) -> Owner {
        let mut admission = self.admission.lock().unwrap_or_else(|e| e.into_inner());
        let mut registry = admission.as_mut();
        let mut path = self.path.clone();
        let parent = self.node().inspection.as_ref().map(|scope| scope.id);
        let node = Arc::new(Node {
            inspection: registry.as_mut().map(|registry| inspection::ScopeInfo {
                id: registry.id(),
                parent,
                name: name.to_string(),
            }),
            cancel: self.node().cancel.child_token(),
            tasks: TaskTracker::new(),
        });
        if let Some(registry) = registry {
            registry.scopes.retain(|_, scope| scope.strong_count() > 0);
            registry
                .scopes
                .insert(node.inspection.as_ref().unwrap().id, Arc::downgrade(&node));
        }
        path.push(node);
        Owner(Self {
            admission: self.admission.clone(),
            path,
            runtime: self.runtime.clone(),
        })
    }

    pub fn token(&self) -> CancellationToken {
        self.node().cancel.clone()
    }

    pub fn cancel(&self) {
        let _lock = self.admission.lock().unwrap_or_else(|e| e.into_inner());
        self.node().cancel.cancel();
        self.node().tasks.close();
    }

    #[track_caller]
    pub fn register(&self) -> Result<Completion, Closed> {
        self.register_named("", Execution::Supervisor)
    }

    #[track_caller]
    pub fn register_named(
        &self,
        name: impl Display,
        execution: Execution,
    ) -> Result<Completion, Closed> {
        let mut admission = self.admission.lock().unwrap_or_else(|e| e.into_inner());
        if self.path.iter().any(|node| node.cancel.is_cancelled()) {
            return Err(Closed);
        }
        let inspection = if let Some(registry) = admission.as_mut() {
            let id = registry.id();
            registry.tasks.insert(
                id,
                inspection::Record {
                    scope: self.node().inspection.as_ref().unwrap().id,
                    name: name.to_string(),
                    execution,
                    started: std::time::Instant::now(),
                    source: std::panic::Location::caller(),
                },
            );
            Some(inspection::Registration {
                registry: self.admission.clone(),
                id,
                _path: self.path.clone(),
            })
        } else {
            None
        };
        Ok(Completion {
            tokens: self.path.iter().map(|node| node.tasks.token()).collect(),
            inspection,
        })
    }

    /// The future must observe cancellation itself, or deliberately finish its work.
    /// No implicit select/abort: dropping a cleanup future can skip process reaping.
    #[track_caller]
    pub fn track<F: Future>(
        &self,
        work: F,
    ) -> Result<impl Future<Output = F::Output> + use<F>, Closed> {
        self.track_named("", Execution::Supervisor, work)
    }

    #[track_caller]
    pub fn track_named<F: Future>(
        &self,
        name: &str,
        execution: Execution,
        work: F,
    ) -> Result<impl Future<Output = F::Output> + use<F>, Closed> {
        let completion = self.register_named(name, execution)?;
        Ok(async move {
            let output = work.await;
            drop(completion);
            output
        })
    }

    /// Schedule registered work. Rejected work is never submitted to the runtime.
    #[track_caller]
    pub fn spawn<F>(&self, work: F) -> Task<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let Some(runtime) = &self.runtime else {
            return Task::rejected(Error::NoExecutor);
        };
        match self.track_named("", Execution::Tokio, work) {
            Ok(work) => Task::running(runtime.spawn(async move { Ok(work.await) })),
            Err(error) => Task::rejected(error.into()),
        }
    }

    /// Disposable I/O: cancellation drops the future; cleanup must be RAII-safe.
    #[track_caller]
    pub fn spawn_cancellable<F>(&self, work: F) -> Task<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let Some(runtime) = &self.runtime else {
            return Task::rejected(Error::NoExecutor);
        };
        let cancel = self.token();
        let work = match self.track_named("", Execution::Tokio, async move {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(Closed.into()),
                output = work => Ok(output),
            }
        }) {
            Ok(work) => work,
            Err(error) => return Task::rejected(error.into()),
        };
        Task::running(runtime.spawn(work))
    }

    /// Finite blocking work remains registered until its closure actually finishes.
    #[track_caller]
    pub fn spawn_blocking<F, T>(&self, work: F) -> Task<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let Some(runtime) = &self.runtime else {
            return Task::rejected(Error::NoExecutor);
        };
        let completion = match self.register_named("", Execution::Blocking) {
            Ok(completion) => completion,
            Err(error) => return Task::rejected(error.into()),
        };
        Task::running(runtime.spawn_blocking(move || {
            let output = work();
            drop(completion);
            Ok(output)
        }))
    }

    /// Drive borrowed async I/O on a dedicated synchronous worker.
    ///
    /// # Errors
    /// Rejects work after shutdown or without an executor. Must not run inside an async task.
    #[track_caller]
    pub fn block_on<F: Future>(&self, work: F) -> Result<F::Output, Error> {
        let runtime = self.runtime.as_ref().ok_or(Error::NoExecutor)?;
        Ok(runtime.block_on(self.track(work)?))
    }

    #[track_caller]
    pub fn spawn_thread<F>(
        &self,
        builder: std::thread::Builder,
        work: F,
    ) -> std::io::Result<std::thread::JoinHandle<()>>
    where
        F: FnOnce() + Send + 'static,
    {
        let completion = self
            .register_named("", Execution::Thread)
            .map_err(std::io::Error::other)?;
        builder.spawn(move || {
            work();
            drop(completion);
        })
    }

    /// Seal admission, notify descendants, and wait for all registered cleanup.
    pub async fn shutdown(&self) {
        self.cancel();
        self.node().tasks.wait().await;
    }
}

// Fields must stay alive until drop, including during unwinding.
impl Drop for Completion {
    fn drop(&mut self) {
        self.inspection.take();
        self.tokens.clear();
    }
}

mod supervisor;
pub use supervisor::Supervisor;
mod group;
pub use group::Group;
mod execution;
pub use execution::{Error, Task};
mod inspection;
pub mod process;
pub use inspection::{Execution, Inspector, ScopeSnapshot, Snapshot, TaskSnapshot};
#[cfg(test)]
mod tests;
