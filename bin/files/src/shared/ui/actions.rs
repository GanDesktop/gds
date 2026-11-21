//! Files command metadata shared by native `zgui` keyboard dispatch and Help.

pub use gds::ui::actions::{
    Cancel, CloseAllNotifications, CloseAllNotifications as ClearNotifications, CloseNotification,
    FocusNotifications,
};
use gds::ui::{Shortcut, actions as ui_actions};
use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey};

const _: ClearNotifications = CloseAllNotifications;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Command {
    Menu,
    Settings,
    Help,
    NewFolder,
    NewTab,
    NewWindow,
    CloseTab,
    NextTab,
    PreviousTab,
    Sidebar,
    Terminal,
    EditLocation,
    Back,
    Forward,
    Parent,
    Refresh,
    Filter,
    Hidden,
    Favorite,
    Open,
    OpenInNewTab,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    CopyPaths,
    SelectAll,
    DeselectAll,
    ContextMenu,
    Rename,
    Details,
    Columns,
    ViewMode,
    Sort,
    Trash,
    DeletePermanent,
    RestoreSelection,
    PurgeSelection,
    EmptyTrash,
    ClearRecent,
    MountVolume,
    UnmountVolume,
    FocusLocations,
    FocusVolumes,
    FocusTabs,
    FocusContent,
    FocusNotifications,
    CloseNotification,
    CloseAllNotifications,
    Cancel,
}

impl Command {
    pub const ALL: [Self; 51] = [
        Self::Menu,
        Self::Settings,
        Self::Help,
        Self::NewFolder,
        Self::NewTab,
        Self::NewWindow,
        Self::CloseTab,
        Self::NextTab,
        Self::PreviousTab,
        Self::Sidebar,
        Self::Terminal,
        Self::EditLocation,
        Self::Back,
        Self::Forward,
        Self::Parent,
        Self::Refresh,
        Self::Filter,
        Self::Hidden,
        Self::Favorite,
        Self::Open,
        Self::OpenInNewTab,
        Self::Copy,
        Self::Cut,
        Self::Paste,
        Self::Undo,
        Self::Redo,
        Self::CopyPaths,
        Self::SelectAll,
        Self::DeselectAll,
        Self::ContextMenu,
        Self::Rename,
        Self::Details,
        Self::Columns,
        Self::ViewMode,
        Self::Sort,
        Self::Trash,
        Self::DeletePermanent,
        Self::RestoreSelection,
        Self::PurgeSelection,
        Self::EmptyTrash,
        Self::ClearRecent,
        Self::MountVolume,
        Self::UnmountVolume,
        Self::FocusLocations,
        Self::FocusVolumes,
        Self::FocusTabs,
        Self::FocusContent,
        Self::FocusNotifications,
        Self::CloseNotification,
        Self::CloseAllNotifications,
        Self::Cancel,
    ];

    #[must_use]
    pub const fn label_id(self) -> &'static str {
        match self {
            Self::Menu => "app-menu",
            Self::Settings => "settings",
            Self::Help => "help",
            Self::NewFolder => "new-folder",
            Self::NewTab => "new-tab",
            Self::NewWindow => "new-window",
            Self::CloseTab => "close-tab",
            Self::NextTab => "next-tab",
            Self::PreviousTab => "previous-tab",
            Self::Sidebar => "sidebar-toggle",
            Self::Terminal => "terminal-toggle",
            Self::EditLocation => "path-edit",
            Self::Back => "back",
            Self::Forward => "forward",
            Self::Parent => "parent",
            Self::Refresh => "refresh",
            Self::Filter => "filter",
            Self::Hidden => "show-hidden",
            Self::Favorite => "favorite-toggle",
            Self::Open => "open",
            Self::OpenInNewTab => "open-in-new-tab",
            Self::Copy => "copy-files",
            Self::Cut => "cut-files",
            Self::Paste => "paste-files",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::CopyPaths => "copy-path",
            Self::SelectAll => "select-all",
            Self::DeselectAll => "deselect-all",
            Self::ContextMenu => "selection-menu",
            Self::Rename => "rename",
            Self::Details => "details-toggle",
            Self::Columns => "columns-configure",
            Self::ViewMode => "view-toggle",
            Self::Sort => "sort",
            Self::Trash => "move-to-trash",
            Self::DeletePermanent => "delete-permanently",
            Self::RestoreSelection => "recycle-restore",
            Self::PurgeSelection => "delete-permanently",
            Self::EmptyTrash => "recycle-empty-action",
            Self::ClearRecent => "clear-recent",
            Self::MountVolume => "device-mount",
            Self::UnmountVolume => "device-unmount",
            Self::FocusLocations => "focus-locations",
            Self::FocusVolumes => "focus-volumes",
            Self::FocusTabs => "focus-tabs",
            Self::FocusContent => "focus-content",
            Self::FocusNotifications => "focus-notifications",
            Self::CloseNotification => "notification-close",
            Self::CloseAllNotifications => "notifications-close-all",
            Self::Cancel => "close",
        }
    }

    /// Primary keyboard shortcut displayed by keycaps, menus, tooltips, and Help.
    #[must_use]
    pub fn shortcut(self) -> Shortcut {
        match self {
            Self::Menu => Shortcut::new(Key::character("m"), Modifiers::ALT),
            Self::Settings => Shortcut::new(Key::character(","), Modifiers::CONTROL),
            Self::Help => Shortcut::new(Key::Named(NamedKey::F1), Modifiers::NONE),
            Self::NewFolder => {
                Shortcut::new(Key::character("n"), Modifiers::CONTROL | Modifiers::SHIFT)
            }
            Self::NewTab => Shortcut::new(Key::character("n"), Modifiers::CONTROL),
            Self::NewWindow => {
                Shortcut::new(Key::character("n"), Modifiers::CONTROL | Modifiers::ALT)
            }
            Self::CloseTab => Shortcut::new(Key::character("w"), Modifiers::CONTROL),
            Self::NextTab => Shortcut::new(Key::Named(NamedKey::Tab), Modifiers::CONTROL),
            Self::PreviousTab => Shortcut::new(
                Key::Named(NamedKey::Tab),
                Modifiers::CONTROL | Modifiers::SHIFT,
            ),
            Self::Sidebar => Shortcut::new(Key::character("b"), Modifiers::CONTROL),
            Self::Terminal => Shortcut::new(Key::character("`"), Modifiers::CONTROL),
            Self::EditLocation => Shortcut::new(Key::character("l"), Modifiers::CONTROL),
            Self::Back => Shortcut::new(Key::Named(NamedKey::ArrowLeft), Modifiers::ALT),
            Self::Forward => Shortcut::new(Key::Named(NamedKey::ArrowRight), Modifiers::ALT),
            Self::Parent => Shortcut::new(Key::Named(NamedKey::ArrowUp), Modifiers::ALT),
            Self::Refresh => Shortcut::new(Key::character("r"), Modifiers::CONTROL),
            Self::Filter => Shortcut::new(Key::character("f"), Modifiers::CONTROL),
            Self::Hidden => Shortcut::new(Key::character("h"), Modifiers::CONTROL),
            Self::Favorite => Shortcut::new(Key::character("d"), Modifiers::CONTROL),
            Self::Open => Shortcut::new(Key::Named(NamedKey::Enter), Modifiers::NONE),
            Self::OpenInNewTab => Shortcut::new(Key::Named(NamedKey::Enter), Modifiers::CONTROL),
            Self::Copy => Shortcut::new(Key::character("c"), Modifiers::CONTROL),
            Self::Cut => Shortcut::new(Key::character("x"), Modifiers::CONTROL),
            Self::Paste => Shortcut::new(Key::character("v"), Modifiers::CONTROL),
            Self::Undo => Shortcut::new(Key::character("z"), Modifiers::CONTROL),
            Self::Redo => Shortcut::new(Key::character("z"), Modifiers::CONTROL | Modifiers::SHIFT),
            Self::CopyPaths => {
                Shortcut::new(Key::character("c"), Modifiers::CONTROL | Modifiers::SHIFT)
            }
            Self::SelectAll => Shortcut::new(Key::character("a"), Modifiers::CONTROL),
            Self::DeselectAll => {
                Shortcut::new(Key::character("a"), Modifiers::CONTROL | Modifiers::SHIFT)
            }
            Self::ContextMenu => Shortcut::new(Key::Named(NamedKey::F10), Modifiers::SHIFT),
            Self::Rename => Shortcut::new(Key::Named(NamedKey::F2), Modifiers::NONE),
            Self::Details => Shortcut::new(Key::character("p"), Modifiers::CONTROL),
            Self::Columns => Shortcut::new(Key::character("v"), Modifiers::ALT | Modifiers::SHIFT),
            Self::ViewMode => Shortcut::new(Key::character("g"), Modifiers::CONTROL),
            Self::Sort => Shortcut::new(Key::character("s"), Modifiers::ALT | Modifiers::SHIFT),
            Self::Trash => Shortcut::new(Key::Named(NamedKey::Delete), Modifiers::NONE),
            Self::DeletePermanent => Shortcut::new(Key::Named(NamedKey::Delete), Modifiers::SHIFT),
            Self::RestoreSelection => Shortcut::new(Key::character("r"), Modifiers::ALT),
            Self::PurgeSelection => Shortcut::new(Key::character("d"), Modifiers::ALT),
            Self::EmptyTrash => {
                Shortcut::new(Key::character("d"), Modifiers::ALT | Modifiers::SHIFT)
            }
            Self::ClearRecent => Shortcut::new(Key::character("c"), Modifiers::ALT),
            Self::MountVolume => Shortcut::new(Key::character("m"), Modifiers::CONTROL),
            Self::UnmountVolume => {
                Shortcut::new(Key::character("m"), Modifiers::CONTROL | Modifiers::SHIFT)
            }
            Self::FocusLocations => Shortcut::new(Key::character("l"), Modifiers::ALT),
            Self::FocusVolumes => Shortcut::new(Key::character("v"), Modifiers::ALT),
            Self::FocusTabs => Shortcut::new(Key::character("t"), Modifiers::ALT),
            Self::FocusContent => Shortcut::new(Key::Named(NamedKey::Space), Modifiers::ALT),
            Self::FocusNotifications => ui_actions::Command::FocusNotifications.default_stroke(),
            Self::CloseNotification => ui_actions::Command::CloseNotification.default_stroke(),
            Self::CloseAllNotifications => {
                ui_actions::Command::CloseAllNotifications.default_stroke()
            }
            Self::Cancel => ui_actions::Command::Cancel.default_stroke(),
        }
    }

    /// Extra compatibility shortcuts accepted alongside [`Self::shortcut`].
    #[must_use]
    pub fn compatibility_shortcuts(self) -> Vec<Shortcut> {
        match self {
            Self::Filter => vec![Shortcut::new(Key::Named(NamedKey::F3), Modifiers::NONE)],
            Self::Terminal => vec![Shortcut::new(Key::Named(NamedKey::F4), Modifiers::NONE)],
            // F5 is the historical refresh key; Ctrl+R is the primary one.
            Self::Refresh => vec![Shortcut::new(Key::Named(NamedKey::F5), Modifiers::NONE)],
            Self::Sidebar => vec![Shortcut::new(Key::Named(NamedKey::F9), Modifiers::NONE)],
            // F10 is the conventional menu key; Alt+M is the primary one.
            Self::Menu => vec![Shortcut::new(Key::Named(NamedKey::F10), Modifiers::NONE)],
            _ => Vec::new(),
        }
    }

    /// Match either [`Self::shortcut`] or any compatibility binding against a key press.
    #[must_use]
    pub fn matches(self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        let primary = self.shortcut();
        if matches_shortcut(&primary, event, modifiers) {
            return true;
        }
        self.compatibility_shortcuts()
            .into_iter()
            .any(|shortcut| matches_shortcut(&shortcut, event, modifiers))
    }

    /// Resolve a workspace command from a `zgui` key event and modifier state.
    #[must_use]
    pub fn from_key_event(event: &KeyEvent, modifiers: Modifiers) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|command| command.matches(event, modifiers))
    }

    #[must_use]
    pub const fn needs_content_focus(self) -> bool {
        matches!(
            self,
            Self::Open
                | Self::OpenInNewTab
                | Self::Copy
                | Self::Cut
                | Self::Paste
                | Self::CopyPaths
                | Self::SelectAll
                | Self::DeselectAll
                | Self::ContextMenu
                | Self::Rename
                | Self::Trash
                | Self::DeletePermanent
                | Self::RestoreSelection
                | Self::PurgeSelection
                | Self::MountVolume
                | Self::UnmountVolume
                | Self::Undo
                | Self::Redo
        )
    }
}

impl From<Command> for Shortcut {
    fn from(command: Command) -> Self {
        command.shortcut()
    }
}

impl From<FocusNotifications> for Command {
    fn from(_: FocusNotifications) -> Self {
        Self::FocusNotifications
    }
}

impl From<CloseNotification> for Command {
    fn from(_: CloseNotification) -> Self {
        Self::CloseNotification
    }
}

impl From<ClearNotifications> for Command {
    fn from(_: ClearNotifications) -> Self {
        Self::CloseAllNotifications
    }
}

impl From<Cancel> for Command {
    fn from(_: Cancel) -> Self {
        Self::Cancel
    }
}

fn matches_shortcut(shortcut: &Shortcut, event: &KeyEvent, modifiers: Modifiers) -> bool {
    shortcut.matches(&event.key, modifiers)
        || (!shortcut.modifiers.held().is_empty()
            && shortcut.matches(&event.key_without_modifiers, modifiers))
}

/// Direct binding for the `position + 1`-th file tab (`Ctrl+1`..`Ctrl+9`).
#[must_use]
pub fn ordinal_shortcut(position: usize) -> Option<Shortcut> {
    (position < 9).then(|| {
        Shortcut::new(
            Key::character((position + 1).to_string()),
            Modifiers::CONTROL,
        )
    })
}

#[cfg(test)]
mod tests {
    use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey};

    use super::{
        Cancel, ClearNotifications, CloseAllNotifications, CloseNotification, Command,
        FocusNotifications, ordinal_shortcut,
    };

    #[test]
    fn notification_commands_and_reexports_bind_alt_n_alt_x_and_alt_shift_x() {
        let _tokens = (
            Cancel,
            FocusNotifications,
            CloseNotification,
            CloseAllNotifications,
            ClearNotifications,
        );

        assert_eq!(
            Command::FocusNotifications.shortcut(),
            gds::ui::Shortcut::new(Key::character("n"), Modifiers::ALT)
        );
        assert_eq!(
            Command::CloseNotification.shortcut(),
            gds::ui::Shortcut::new(Key::character("x"), Modifiers::ALT)
        );
        assert_eq!(
            Command::CloseAllNotifications.shortcut(),
            gds::ui::Shortcut::new(Key::character("x"), Modifiers::ALT | Modifiers::SHIFT)
        );

        assert_eq!(
            Command::from_key_event(&KeyEvent::character("x"), Modifiers::ALT),
            Some(Command::CloseNotification)
        );
        assert_eq!(
            Command::from_key_event(&KeyEvent::character("X"), Modifiers::ALT | Modifiers::SHIFT),
            Some(Command::CloseAllNotifications)
        );
        assert_eq!(
            Command::from_key_event(&KeyEvent::character("c"), Modifiers::ALT),
            Some(Command::ClearRecent)
        );
        assert_eq!(
            Command::from_key_event(&KeyEvent::character("C"), Modifiers::ALT | Modifiers::SHIFT),
            None
        );
        assert_eq!(
            Command::from_key_event(
                &KeyEvent::character(" "),
                Modifiers::ALT | Modifiers::CAPS_LOCK
            ),
            Some(Command::FocusContent)
        );
    }

    #[test]
    fn ordinal_tab_shortcuts_cover_one_through_nine_only() {
        assert_eq!(
            ordinal_shortcut(0),
            Some(gds::ui::Shortcut::new(
                Key::character("1"),
                Modifiers::CONTROL
            ))
        );
        assert_eq!(
            ordinal_shortcut(8),
            Some(gds::ui::Shortcut::new(
                Key::character("9"),
                Modifiers::CONTROL
            ))
        );
        assert_eq!(ordinal_shortcut(9), None);
        assert_eq!(Command::Help.shortcut().key, Key::Named(NamedKey::F1));
    }
}
