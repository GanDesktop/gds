//! Native Files workspace and its window-owned application state.
mod chrome;
mod drag;
mod enrichment;
mod geometry;
mod keyboard;
mod marquee;
mod menus;
mod model;
mod name_form;
mod navigation;
mod operations;
mod picker;
mod status;
mod surfaces;
mod ui;
mod view;

pub(crate) use ui::{Browser, LaunchOptions, WeakBrowser};
mod advanced;

pub(crate) mod inspection;
mod terminals;

#[cfg(feature = "files-e2e")]
pub(crate) mod e2e;
