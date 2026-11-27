//! Native Files workspace and its window-owned application state.

mod navigation;
mod operations;
mod panes;
mod projection;
mod shell;
mod state;

#[cfg(feature = "files-e2e")]
pub(crate) mod e2e;

pub(crate) use shell::{Browser, WeakBrowser};
pub(crate) use state::LaunchOptions;
