//! Persisted user preferences for GDS Images.

mod api;
mod model;
mod presentation;
mod service;

pub use api::read_startup;
pub use model::Preferences;
pub(crate) use presentation::normalize;
pub use service::Service;
