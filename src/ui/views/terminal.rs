//! Reusable interactive terminal controller and view over the PTY service.

mod geometry;
mod input;
mod instance;
mod palette;
mod preferences;
mod view;

pub use crate::service::terminal::{Shutdown, Status};
pub use instance::Terminal;
pub use preferences::{Preferences, PreferencesEditor};
pub use view::{TerminalView, TerminalViewProps};

/// Keybinding context identifier for terminal keymaps.
pub const INPUT_CONTEXT: &str = "GdsTerminal";

/// Terminal copy action marker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Copy;

/// Terminal paste action marker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Paste;

/// Terminal scrollback page-up action marker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScrollPageUp;

/// Terminal scrollback page-down action marker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScrollPageDown;

/// Terminal scrollback jump-to-top action marker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScrollTop;

/// Terminal scrollback jump-to-bottom action marker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScrollBottom;

/// Observable terminal lifecycle and process title events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Changed,
    TitleChanged(String),
    Bell,
    Closed(Option<i32>),
}
