//! Persisted application preferences and their editor model.
mod api;
mod model;
mod presentation;
mod service;

pub use api::load;
pub use model::{Preferences, PreferencesEditor};
pub(crate) use service::*;
