//! Shared presentation services independent of native window ownership.
use crate::service::logs::Store;

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
    pub fn time(&self) -> Time {
        Time
    }
}
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
pub struct Time;
impl Time {
    pub fn format(&self) -> crate::ui::time::Format {
        crate::ui::time::format()
    }
    pub fn now(&self) -> std::time::SystemTime {
        crate::ui::time::now()
    }
    pub fn apply(&self, format: crate::ui::time::Format) {
        crate::ui::time::apply(format);
    }
}
