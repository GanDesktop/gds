//! Application-wide storage discovery and operations. Live device state never
//! enters the saved places or preferences.

mod model;
mod network;
mod service;
#[cfg(test)]
mod tests;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod udisks;
mod usage;

pub use model::*;
pub use network::read_network_mounts;
pub(crate) use service::*;
pub use usage::CapacityState;
