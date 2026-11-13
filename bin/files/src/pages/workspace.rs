//! The Files window, composing browsing, settings and picker interactions.
#[cfg(feature = "files-e2e")]
pub(crate) mod e2e;
mod ui;
pub(crate) use ui::{Browser, LaunchOptions, NewWindowRequested};
