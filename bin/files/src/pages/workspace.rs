//! Native Files workspace and its window-owned application state.

mod advanced;
mod chrome;
mod drag;
mod enrichment;
mod geometry;
pub(crate) mod inspection;
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
mod terminals;
mod typeahead;
mod ui;
mod view;

#[cfg(feature = "files-e2e")]
pub(crate) mod e2e;

pub(crate) use ui::{Browser, LaunchOptions, WeakBrowser};
