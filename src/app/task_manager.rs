//! Main-thread delivery and owner-bound scopes for supervised application I/O.
use crate::service::task_manager::{self, Scope};
use anti_webui::prelude::{Task, on_cleanup};
use std::future::Future;

#[derive(Clone)]
pub struct Tasks {
    scope: Scope,
}

impl Tasks {
    pub(super) fn new(scope: Scope) -> Self {
        Self { scope }
    }
    pub fn scope(&self) -> Scope {
        self.scope.clone()
    }
    pub fn inspector(&self) -> Option<task_manager::Inspector> {
        self.scope.inspector()
    }
    pub fn is_shutting_down(&self) -> bool {
        self.scope.token().is_cancelled()
    }

    /// Derive I/O ownership from the current window or reactive subtree.
    pub fn owner_scope(&self, name: &str) -> Scope {
        let owner = self.scope.named_child(name);
        let scope = (*owner).clone();
        on_cleanup(move || drop(owner));
        scope
    }

    /// Run disposable work on Tokio and deliver its outcome under the current UI owner.
    /// Work requiring asynchronous cleanup should instead use `Scope::spawn_named`.
    pub fn spawn<F>(&self, name: &str, work: F) -> Task<Result<F::Output, task_manager::Error>>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let (result, complete) = Task::pending();
        let scope = self.owner_scope(name);
        scope
            .spawn_cancellable_named(name, work)
            .on_complete(&scope, move |value| complete.complete(value));
        result
    }

    pub fn request_quit(&self) {
        anti_webui::prelude::quit();
    }
}
