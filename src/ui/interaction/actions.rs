//! Suite-wide decisions and notification navigation. Applications own their commands.
use gpui_kit::*;

gpui_kit::actions!(
    gds,
    [
        FocusNotifications,
        CloseNotification,
        CloseAllNotifications,
        Cancel
    ]
);

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
    pub fn action(self) -> Box<dyn Action> {
        match self {
            Self::FocusNotifications => Box::new(FocusNotifications),
            Self::CloseNotification => Box::new(CloseNotification),
            Self::CloseAllNotifications => Box::new(CloseAllNotifications),
            Self::Cancel => Box::new(Cancel),
        }
    }
    pub fn context(self) -> Option<&'static str> {
        None
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

pub fn register(cx: &mut App) {
    let bindings = Command::ALL
        .into_iter()
        .map(|command| {
            KeyBinding::load(
                command.default_stroke(),
                command.action(),
                None,
                false,
                None,
                cx,
            )
            .expect("suite shortcuts are valid")
        })
        .collect::<Vec<_>>();
    cx.bind_keys(bindings);
    crate::ui::dialog::register(cx);
}
