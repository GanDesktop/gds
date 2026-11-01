//! The desktop workspace and its native window host.
mod model;
mod ui;
pub(crate) use model::routes;
pub(crate) use model::windows;
pub(crate) use ui::{Event, TERMINAL_TABS_CONTEXT, Workspace, bind_keys, initial_size};
