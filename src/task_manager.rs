//! Executor-independent ownership, cooperative cancellation and completion tracking.
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
}

impl Default for Scope {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Scope {
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

    #[track_caller]
    pub fn spawn<F>(
        &self,
        runtime: &tokio::runtime::Handle,
        work: F,
    ) -> Result<tokio::task::JoinHandle<F::Output>, Closed>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        Ok(runtime.spawn(self.track_named("", Execution::Tokio, work)?))
    }

    #[track_caller]
    pub fn spawn_blocking<F, T>(
        &self,
        runtime: &tokio::runtime::Handle,
        work: F,
    ) -> Result<tokio::task::JoinHandle<T>, Closed>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let completion = self.register_named("", Execution::Blocking)?;
        Ok(runtime.spawn_blocking(move || {
            let output = work();
            drop(completion);
            output
        }))
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

mod inspection;
pub mod process;
pub use inspection::{Execution, ScopeSnapshot, Snapshot, TaskSnapshot};
#[cfg(test)]
mod tests;
