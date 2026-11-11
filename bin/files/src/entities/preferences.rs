//! Persisted application preferences and their editor model.
mod api;
mod model;
mod presentation;
mod service;
pub(crate) use api::load;
pub(crate) use model::{Preferences, PreferencesEditor};
pub(crate) use service::{Current, PreferencesExt, SaveFailed};
