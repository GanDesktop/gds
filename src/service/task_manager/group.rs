//! A service-owned set of cancellable child tasks, all admitted through its scope.
use super::{Execution, Scope};
use std::future::Future;
use tokio::task::{AbortHandle, JoinError, JoinSet};

pub struct Group<T> {
    scope: Scope,
    tasks: JoinSet<T>,
}
impl<T: Send + 'static> Group<T> {
    pub fn new(scope: Scope) -> Self {
        Self {
            scope,
            tasks: JoinSet::new(),
        }
    }
    #[track_caller]
    pub fn spawn<F>(&mut self, work: F) -> Option<AbortHandle>
    where
        F: Future<Output = T> + Send + 'static,
    {
        self.spawn_named("", work)
    }
    #[track_caller]
    pub fn spawn_named<F>(&mut self, name: &str, work: F) -> Option<AbortHandle>
    where
        F: Future<Output = T> + Send + 'static,
    {
        let runtime = self.scope.runtime.as_ref()?;
        let work = self.scope.track_named(name, Execution::Tokio, work).ok()?;
        Some(self.tasks.spawn_on(work, runtime))
    }
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
    pub fn abort_all(&mut self) {
        self.tasks.abort_all();
    }
    pub async fn join_next(&mut self) -> Option<Result<T, JoinError>> {
        self.tasks.join_next().await
    }
}
