//! UI-thread task delivery and owner-bound scopes for supervised application I/O.
use crate::service::task_manager::{self, Scope};
use std::future::Future;
use zgui::reactive::{Owner, on_cleanup_local, spawn_local};
use zgui::runtime::windows::try_use_windows;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct Tasks {
    scope: Scope,
    owner: Owner,
}

impl Tasks {
    pub(super) fn new(scope: Scope, owner: Owner) -> Self {
        Self { scope, owner }
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

    /// Derive I/O ownership from the current UI component or window reactive owner.
    pub fn owner_scope(&self, name: &str) -> Scope {
        let child = self.scope.named_child(name);
        let scope = (*child).clone();
        if Owner::current().is_some() {
            on_cleanup_local(move || drop(child));
        } else {
            self.owner.with(|| on_cleanup_local(move || drop(child)));
        }
        scope
    }

    /// Run cancellable work on Tokio bound to the current UI reactive owner.
    #[track_caller]
    pub fn spawn<F>(&self, name: &str, work: F) -> task_manager::Task<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.owner_scope(name).spawn_cancellable_named(name, work)
    }

    /// Run work on Tokio and deliver its result on the UI thread under the current owner.
    #[track_caller]
    pub fn spawn_ui<F, C>(&self, name: &str, work: F, on_complete: C) -> zgui::reactive::Task
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
        C: FnOnce(Result<F::Output, task_manager::Error>) + 'static,
    {
        let task = self.spawn(name, work);
        let launch = move || {
            spawn_local(async move {
                on_complete(task.await);
            })
        };
        if Owner::current().is_some() {
            launch()
        } else {
            self.owner.with(launch)
        }
    }

    /// Cancel application tasks and ask active `zgui` windows to quit.
    pub fn request_quit(&self) {
        self.scope.cancel();
        if let Some(windows) = try_use_windows() {
            windows.quit();
        }
    }
}
