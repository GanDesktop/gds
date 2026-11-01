//! Terminal preferences use Agents' own section-scoped configuration writer.

use gds::ui::terminal::Preferences;
use gpui_kit::{App, Global};

#[derive(Clone, Default)]
pub(crate) struct Current(pub Preferences);
impl Global for Current {}

pub(crate) fn init(cx: &mut App) {
    if cx.try_global::<super::Configuration>().is_none() {
        cx.set_global(Current::default());
        return;
    }
    let preferences = match super::store(cx).load() {
        Ok(document) => document.settings.terminal,
        Err(error) => {
            tracing::error!(%error, "cannot load local terminal preferences");
            Preferences::default()
        }
    };
    cx.set_global(Current(preferences));
}

pub(crate) fn current(cx: &App) -> Preferences {
    cx.try_global::<Current>()
        .map(|current| current.0.clone())
        .unwrap_or_default()
}
