//! Desktop capabilities and their independently usable process protocol.

#[cfg(feature = "desktop-protocol")]
pub mod protocol;
#[cfg(feature = "watch")]
pub mod watch;

#[cfg(feature = "desktop")]
mod client;
#[cfg(feature = "desktop")]
mod error;
#[cfg(feature = "desktop")]
pub use client::{Desktop, OpenMode, OpenOutcome};
#[cfg(feature = "desktop")]
pub use error::Error;
#[cfg(all(feature = "desktop", target_os = "linux"))]
pub mod file_manager;
#[cfg(all(feature = "desktop", target_os = "linux"))]
mod portal;
#[cfg(all(feature = "desktop", target_os = "linux"))]
pub use portal::Error as PortalError;
