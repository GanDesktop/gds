//! Window-owned terminal sessions with disposable native views.
mod instance;
mod preferences;
mod view;
pub use crate::service::terminal::{Shutdown, Status};
pub use instance::Terminal;
pub use preferences::{Preferences, PreferencesEditor};
