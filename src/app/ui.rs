//! Shared desktop startup assembly, available only with the UI capability.
use super::Gds;
use crate::service::logs::Store;

impl Gds<'_> {
    /// Install task ownership before UI services, with optional diagnostics.
    pub fn initialize(&mut self, scope: crate::service::task_manager::Scope, logs: Option<Store>) {
        self.tasks().init(scope);
        crate::appearance::init(self.cx);
        if let Some(logs) = logs {
            crate::ui::logs::install(logs, self.cx);
        }
    }
}
