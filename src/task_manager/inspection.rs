//! Read-only task snapshots. Observing work never owns or cancels it.
use super::*;
use std::{
    collections::BTreeMap,
    panic::Location,
    sync::Weak,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Execution {
    Supervisor,
    GpuiBackground,
    Tokio,
    Blocking,
    Thread,
}

#[derive(Clone, Debug)]
pub struct ScopeSnapshot {
    pub id: u64,
    pub parent: Option<u64>,
    pub name: String,
    pub cancelling: bool,
}

#[derive(Clone, Debug)]
pub struct TaskSnapshot {
    pub id: u64,
    pub scope: u64,
    pub name: String,
    pub execution: Execution,
    pub cancelling: bool,
    pub age: Duration,
    pub source: &'static Location<'static>,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub scopes: Vec<ScopeSnapshot>,
    pub tasks: Vec<TaskSnapshot>,
}

pub(super) struct Record {
    pub scope: u64,
    pub name: String,
    pub execution: Execution,
    pub started: Instant,
    pub source: &'static Location<'static>,
}

pub(super) struct ScopeInfo {
    pub id: u64,
    pub parent: Option<u64>,
    pub name: String,
}

pub(super) struct Registration {
    pub registry: Arc<Mutex<Option<Registry>>>,
    pub id: u64,
    // Cancelled scopes remain visible until their final resource is cleaned up.
    pub _path: Vec<Arc<Node>>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(registry) = self
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            registry.tasks.remove(&self.id);
        }
    }
}

#[derive(Default)]
pub(super) struct Registry {
    pub next: u64,
    pub scopes: BTreeMap<u64, Weak<Node>>,
    pub tasks: BTreeMap<u64, Record>,
}

impl Registry {
    pub fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

impl Scope {
    /// Snapshot metadata only; dead scopes and completed work are not retained.
    pub fn snapshot(&self) -> Snapshot {
        let mut admission = self.admission.lock().unwrap_or_else(|e| e.into_inner());
        let Some(registry) = admission.as_mut() else {
            return Snapshot::default();
        };
        registry.scopes.retain(|_, scope| scope.strong_count() > 0);
        let scopes: Vec<_> = registry
            .scopes
            .iter()
            .filter_map(|(&id, scope)| {
                let scope = scope.upgrade()?;
                let info = scope.inspection.as_ref()?;
                Some(ScopeSnapshot {
                    id,
                    parent: info.parent,
                    name: info.name.clone(),
                    cancelling: scope.cancel.is_cancelled(),
                })
            })
            .collect();
        let tasks = registry
            .tasks
            .iter()
            .map(|(&id, task)| TaskSnapshot {
                id,
                scope: task.scope,
                name: task.name.clone(),
                execution: task.execution,
                cancelling: scopes
                    .iter()
                    .any(|scope| scope.id == task.scope && scope.cancelling),
                age: task.started.elapsed(),
                source: task.source,
            })
            .collect();
        Snapshot { scopes, tasks }
    }
}
