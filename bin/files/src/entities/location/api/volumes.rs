//! Application-wide storage discovery and operations. Live device state never
//! enters the saved places or preferences.

#[cfg(target_os = "linux")]
mod linux;
mod model;
mod service;
mod usage;

pub use model::*;
pub use service::*;
pub use usage::{Capacity, CapacityState};

#[cfg(test)]
mod tests;
