//! Runtime capabilities consumed by application code and UI adapters.
#[cfg(any(feature = "desktop", feature = "remote"))]
mod window_id;
#[cfg(any(feature = "desktop", feature = "remote"))]
pub use window_id::{ParentWindow, ParseParentWindowError};
#[cfg(any(feature = "desktop", feature = "desktop-protocol", feature = "watch"))]
pub mod desktop;
#[cfg(feature = "file-inspection")]
pub mod file_inspection;
#[cfg(feature = "file-tags")]
pub mod file_tags;
#[cfg(feature = "image-engine")]
pub mod image;
#[cfg(feature = "logs")]
pub mod logs;
#[cfg(feature = "remote")]
pub mod remote;
#[cfg(feature = "storage")]
pub mod storage;
#[cfg(feature = "task-manager")]
pub mod task_manager;
#[cfg(feature = "terminal")]
pub mod terminal;
