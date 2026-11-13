//! Application-wide storage discovery and operations. Live device state never
//! enters the saved places or preferences.

#[cfg(target_os = "linux")]
mod linux;
mod model;
mod network;
mod service;
#[cfg(test)]
mod tests;
mod usage;

pub use model::*;
pub use network::{NetworkMountError, read_network_mounts};
pub(crate) use service::*;
pub use usage::{Capacity, CapacityState};
