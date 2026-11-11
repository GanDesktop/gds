//! Saved locations and live storage-volume discovery.
mod api;
mod model;
mod ui;
pub(crate) use api::volumes;
pub(crate) mod places;

pub(crate) use model::address::uri::Error as AddressError;
pub(crate) use model::address::{Location, ViewScope, VolumeIdentity};
pub(crate) use model::view::{ColumnWidths, ViewSettings};
pub(crate) use ui::{Collection, LocationTarget};

pub(crate) use model::navigation;

pub(crate) mod scheme;
pub(crate) mod x;
