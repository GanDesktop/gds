use super::{Application, Tasks};
use crate::service::task_manager::Scope;
use gpui_kit::{App, Global};

struct Executor {
    scope: Scope,
    runtime: Option<tokio::runtime::Runtime>,
}
impl Global for Executor {}
impl Drop for Executor {
    fn drop(&mut self) {
        self.scope.cancel();
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl Tasks<&mut App> {
    /// Install an isolated executor for a GPUI test application.
    pub fn init_test(&mut self, inspect: bool) {
        if self.cx.has_global::<Application>() {
            return;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("test executor");
        let scope = Scope::with_runtime(&runtime, inspect);
        self.cx.set_global(Executor {
            scope: scope.clone(),
            runtime: Some(runtime),
        });
        self.init(scope);
        self.ready(false);
    }
}
