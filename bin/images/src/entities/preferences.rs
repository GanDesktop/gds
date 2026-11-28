//! Persisted user preferences for GDS Images.

mod api;
mod model;
mod presentation;
mod service;

pub use api::read_startup;
#[cfg(test)]
pub(crate) use api::read_startup_from;
pub use model::Preferences;
pub(crate) use presentation::normalize;
pub use service::Service;
