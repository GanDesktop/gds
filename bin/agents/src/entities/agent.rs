//! Agent catalogs, installation checks and metadata scoped to a backend.
mod api;
mod model;
mod ui;
pub(crate) use api::{catalog, check, metadata};
pub(crate) use model::{Changes, Status, init};
pub(crate) use ui::icon;
