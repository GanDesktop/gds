//! Shared presentation services independent of native window ownership.
use crate::service::logs::Store;

/// Application UI service registry anchored to [`Gds`](super::Gds).
pub struct Ui {
    logs: Logs,
}

impl Ui {
    pub(super) fn new(store: Option<Store>) -> Self {
        Self {
            logs: Logs { store },
        }
    }

    pub fn logs(&self) -> &Logs {
        &self.logs
    }
}

/// Access to the process-wide diagnostic log collector when enabled.
pub struct Logs {
    store: Option<Store>,
}

impl Logs {
    pub fn is_enabled(&self) -> bool {
        self.store.is_some()
    }

    pub fn store(&self) -> Option<Store> {
        self.store.clone()
    }
}
