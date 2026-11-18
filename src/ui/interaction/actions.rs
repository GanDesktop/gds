//! Suite-wide command metadata; applications own their event routing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    FocusNotifications,
    CloseNotification,
    CloseAllNotifications,
    Cancel,
}

impl Command {
    pub const ALL: [Self; 4] = [
        Self::FocusNotifications,
        Self::CloseNotification,
        Self::CloseAllNotifications,
        Self::Cancel,
    ];
    pub fn label_id(self) -> &'static str {
        match self {
            Self::FocusNotifications => "focus-notifications",
            Self::CloseNotification => "notification-close",
            Self::CloseAllNotifications => "notifications-close-all",
            Self::Cancel => "close",
        }
    }
    pub fn default_stroke(self) -> &'static str {
        match self {
            Self::FocusNotifications => "alt-n",
            Self::CloseNotification => "alt-x",
            Self::CloseAllNotifications => "alt-shift-x",
            Self::Cancel => "escape",
        }
    }
}
