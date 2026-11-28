//! Desktop capabilities and their independently usable process protocol.

#[cfg(feature = "desktop-protocol")]
pub mod protocol;
#[cfg(feature = "watch")]
pub mod watch;

#[cfg(feature = "desktop")]
mod client;
#[cfg(feature = "desktop")]
mod error;
#[cfg(all(feature = "desktop", unix))]
mod executable;
#[cfg(feature = "desktop")]
pub mod file_transfer;
#[cfg(feature = "desktop")]
pub mod recent_files;
#[cfg(feature = "desktop")]
mod session;
#[cfg(feature = "desktop")]
pub use crate::service::ParentWindow;
#[cfg(feature = "desktop")]
pub use client::Desktop;
#[cfg(feature = "desktop")]
pub use error::Error;

#[cfg(feature = "desktop")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    DefaultApplication,
    ChooseApplication,
}

/// Optional parent-window and Wayland activation context forwarded to XDG portals.
#[cfg(feature = "desktop")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenContext {
    pub parent_window: Option<ParentWindow>,
    pub activation_token: Option<String>,
}

#[cfg(feature = "desktop")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenOutcome {
    Opened,
    Cancelled,
}
#[cfg(feature = "desktop")]
pub use session::{
    DEFAULT_INHIBIT_DELAY, DesktopNotification, InhibitRelease, NotificationUrgency,
    hold_inhibit_after, hold_inhibit_after_with_window, send_notification,
};
#[cfg(all(feature = "desktop", feature = "task-manager"))]
pub use session::{InhibitGuard, notify_background};
#[cfg(all(feature = "desktop", any(target_os = "linux", target_os = "freebsd")))]
pub mod file_manager;
#[cfg(all(feature = "desktop", any(target_os = "linux", target_os = "freebsd")))]
mod portal;
#[cfg(all(feature = "desktop", any(target_os = "linux", target_os = "freebsd")))]
pub use portal::Error as PortalError;
