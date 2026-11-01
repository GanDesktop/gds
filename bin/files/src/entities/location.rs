//! Saved locations and live storage-volume discovery.
mod api;
mod model;
pub(crate) use api::volumes;
pub(crate) mod places;

pub(crate) use model::address::{Location, ViewScope, VolumeIdentity};
pub(crate) use model::view::{ColumnWidths, ViewSettings};

pub(crate) use model::navigation;
