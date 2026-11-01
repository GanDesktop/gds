//! Shared foundation for the GDS desktop applications.

extern crate self as gds;
#[cfg(feature = "ui")]
pub use gds_settings_derive::ConfigEditor;

#[cfg(feature = "appearance")]
pub mod appearance;
#[cfg(feature = "i18n")]
pub mod i18n;
#[cfg(feature = "persistence")]
pub mod persistence;
pub mod service;
#[cfg(feature = "ui")]
pub mod ui;
