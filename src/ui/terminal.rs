//! A reusable single-terminal process and surface, independent of host layout or key maps.
mod input;
mod instance;
mod preferences;
mod view;

pub use crate::service::terminal::{Shutdown, Status};
pub use instance::Terminal;
pub use preferences::{Preferences, PreferencesEditor};

use gpui_kit::{App, Global, KeyBinding, NoAction, actions};

pub const INPUT_CONTEXT: &str = "GdsTerminal";

/// Observable process state/title change. Host navigation is not a terminal event.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Changed,
}

// Capabilities, not a key map. Applications choose if and where to bind them.
actions!(
    terminal,
    [
        Copy,
        Paste,
        ScrollPageUp,
        ScrollPageDown,
        ScrollTop,
        ScrollBottom
    ]
);

struct Initialized;
impl Global for Initialized {}

/// Reserve Tab for PTY input. Applications own service cleanup and other shortcuts.
pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<Initialized>() {
        return;
    }
    cx.set_global(Initialized);
    // Root's accessibility bindings run before key_down. Unbind them only on
    // the PTY focus path so the terminal encoder receives Tab / Backtab.
    cx.bind_keys([
        KeyBinding::new("tab", NoAction, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-tab", NoAction, Some(INPUT_CONTEXT)),
    ]);
}
