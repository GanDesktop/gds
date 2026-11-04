//! Persisted application preferences and their editor model.
mod api;
mod model;
pub(crate) use api::{Current, current, load, publish, save};
#[cfg(test)]
pub(crate) use model::FilesPreferences;
pub(crate) use model::{Preferences, PreferencesEditor};
