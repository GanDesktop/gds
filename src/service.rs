//! Runtime capabilities consumed by application code and UI adapters.
#[cfg(any(feature = "desktop", feature = "desktop-protocol", feature = "watch"))]
pub mod desktop;
#[cfg(feature = "file-inspection")]
pub mod file_inspection;
#[cfg(feature = "logs")]
pub mod logs;
#[cfg(feature = "remote")]
pub mod remote;
#[cfg(feature = "storage")]
pub mod storage;
#[cfg(feature = "terminal")]
pub mod terminal;
