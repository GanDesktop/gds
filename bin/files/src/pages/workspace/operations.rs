//! Layer 4 workspace file, transfer, remote, device, tag, and picker operations.

mod devices;
mod dispatch;
mod lifecycle;
mod local;
mod paste;
pub(in crate::pages::workspace) mod picker;
mod remote;
mod tags;

pub(in crate::pages::workspace) use devices::DeviceOps;
pub(in crate::pages::workspace) use dispatch::Dispatch;
pub(in crate::pages::workspace) use lifecycle::Lifecycle;
pub(in crate::pages::workspace) use local::LocalOps;
pub(in crate::pages::workspace) use paste::PasteOps;
pub(in crate::pages::workspace) use picker::PickerOps;
pub(in crate::pages::workspace) use remote::RemoteOps;
pub(in crate::pages::workspace) use tags::TagOps;

#[cfg(test)]
mod tests;
