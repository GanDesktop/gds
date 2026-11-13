//! Headless terminal sessions, lifecycle management and protocol encoding.
mod backend;
mod directory;
pub mod input;
mod lifecycle;
mod session;

pub use lifecycle::{Launch, Shutdown, Status, Terminal};
pub use session::{Error, Session, SpawnOptions, Status as ProcessStatus};
