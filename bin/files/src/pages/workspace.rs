//! The Files window, composing browsing, settings and picker interactions.
mod ui;
#[cfg(feature = "files-e2e")]
pub(crate) use ui::e2e;
pub(crate) use ui::{Browser, LaunchOptions, NewWindowRequested};
