//! Headless terminal sessions, lifecycle management and protocol encoding.
mod backend;
mod directory;
pub mod input;
mod lifecycle;
mod session;
mod state;

pub use lifecycle::{Launch, Shutdown, Status, Terminal};
pub use session::Session;
pub use state::{Error, SpawnOptions, Status as ProcessStatus};
