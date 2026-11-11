//! Owns the application I/O executor; services receive only scopes.
use super::{Error, Scope};
use std::future::Future;

pub struct Supervisor {
    scope: Scope,
    _runtime: tokio::runtime::Runtime,
}

impl Supervisor {
    /// # Errors
    /// Reports failure to create the application I/O runtime.
    pub fn new(inspect: bool) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("gds-io")
            .enable_all()
            .build()?;
        Ok(Self {
            scope: Scope::with_runtime(&runtime, inspect),
            _runtime: runtime,
        })
    }

    pub fn scope(&self) -> Scope {
        self.scope.clone()
    }

    /// Drive bootstrap work before entering the application's main-thread loop.
    ///
    /// # Errors
    /// Rejects work once shutdown has started. Must not run inside an async task.
    #[track_caller]
    pub fn block_on<F: Future>(&self, work: F) -> Result<F::Output, Error> {
        self.scope.block_on(work)
    }

    /// Drive bootstrap I/O with a diagnostic name.
    ///
    /// # Errors
    /// Rejects work after shutdown. Must not run inside an async task.
    #[track_caller]
    pub fn block_on_named<F: Future>(&self, name: &str, work: F) -> Result<F::Output, Error> {
        self.scope.block_on_named(name, work)
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.scope.cancel();
    }
}
