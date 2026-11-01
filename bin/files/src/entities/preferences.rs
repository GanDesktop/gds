//! Persisted application preferences and their editor model.
mod api;
mod model;
pub(crate) use api::{load_startup, save};
pub(crate) use model::{
    ApplyError, FilesPreferences, Preferences, PreferencesEditor, SidebarPreferences,
};
