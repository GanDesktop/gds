//! The selected configuration store is shared by all frontend features.

use std::path::PathBuf;

pub mod appearance;
mod connections;
pub(crate) mod store;
pub(crate) mod terminal;
pub(crate) use connections::{ConnectionProfile, Frontend};
use gpui_kit::{App, Global};
pub(crate) use store::{Document, Error, Store};

pub(crate) struct Configuration(Store);
impl Global for Configuration {}

pub(crate) fn init(settings_file: Option<PathBuf>, cx: &mut App) {
    let settings_file = settings_file.unwrap_or_else(|| {
        directories::BaseDirs::new()
            .map(|dirs| dirs.config_dir().to_owned())
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("gds/agents/settings.json")
    });
    cx.set_global(Configuration(Store::new(Some(settings_file))));
}

pub(crate) fn store(cx: &App) -> Store {
    cx.global::<Configuration>().0.clone()
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

pub(crate) const TERMINAL_TABS_CONTEXT: &str = "AgentsTerminalTabs";
