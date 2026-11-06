//! Provider-defined workspace launch forms, bound to one explicit backend connection.
mod api;
mod model;
mod ui;

pub(crate) use ui::{ChooseDirectory, Panel as Launch, open};
