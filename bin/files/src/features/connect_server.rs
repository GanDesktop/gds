//! Connection workflow and its GPUI authentication prompts.
mod model;
mod ui;
pub(crate) use model::Intent;
pub(crate) use ui::{Connected, Connector, Failed};
