//! Persisted application preferences and their editor model.
mod api;
mod model;
mod presentation;
mod service;

pub use api::{StartupRead, read_startup};
pub use model::{FilesPreferences, Preferences, ThumbnailerCommands};
pub(crate) use service::*;
