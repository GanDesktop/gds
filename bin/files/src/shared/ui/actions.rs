//! Files command metadata shared by native dispatch and Help.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

    pub fn label_id(self) -> &'static str {
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

    /// The default keystroke for this command, used by the workspace.
    pub fn default_stroke(self) -> &'static str {
        match self {
            Self::Menu => "alt-m",
            Self::Settings => "ctrl-,",
            Self::Help => "f1",
            Self::NewFolder => "ctrl-shift-n",
            Self::NewTab => "ctrl-n",
            Self::NewWindow => "ctrl-alt-n",
            Self::CloseTab => "ctrl-w",
            Self::NextTab => "ctrl-tab",
            Self::PreviousTab => "ctrl-shift-tab",
            Self::Sidebar => "ctrl-b",
            Self::Terminal => "ctrl-`",
            Self::EditLocation => "ctrl-l",
            Self::Back => "alt-left",
            Self::Forward => "alt-right",
            Self::Parent => "alt-up",
            Self::Refresh => "ctrl-r",
            Self::Filter => "ctrl-f",
            Self::Hidden => "ctrl-h",
            Self::Favorite => "ctrl-d",
            Self::Open => "enter",
            Self::OpenInNewTab => "ctrl-enter",
            Self::Copy => "ctrl-c",
            Self::Cut => "ctrl-x",
            Self::Paste => "ctrl-v",
            Self::Undo => "ctrl-z",
            Self::Redo => "ctrl-shift-z",
            Self::CopyPaths => "ctrl-shift-c",
            Self::SelectAll => "ctrl-a",
            Self::DeselectAll => "ctrl-shift-a",
            Self::ContextMenu => "shift-f10",
            Self::Rename => "f2",
            Self::Details => "ctrl-p",
            Self::Columns => "alt-shift-v",
            Self::ViewMode => "ctrl-g",
            Self::Sort => "alt-shift-s",
            Self::Trash => "delete",
            Self::DeletePermanent => "shift-delete",
            Self::RestoreSelection => "alt-r",
            Self::PurgeSelection => "alt-d",
            Self::EmptyTrash => "alt-shift-d",
            Self::ClearRecent => "alt-c",
            Self::MountVolume => "ctrl-m",
            Self::UnmountVolume => "ctrl-shift-m",
            Self::FocusLocations => "alt-l",
            Self::FocusVolumes => "alt-v",
            Self::FocusTabs => "alt-t",
            Self::FocusContent => "alt-space",
            Self::FocusNotifications => {
                gds::ui::actions::Command::FocusNotifications.default_stroke()
            }
            Self::CloseNotification => {
                gds::ui::actions::Command::CloseNotification.default_stroke()
            }
            Self::CloseAllNotifications => {
                gds::ui::actions::Command::CloseAllNotifications.default_stroke()
            }
            Self::Cancel => gds::ui::actions::Command::Cancel.default_stroke(),
        }
    }

    /// Extra strokes that run the same action.
    ///
    /// The workspace accepts these alongside [`Self::default_stroke`], so the primary
    /// stroke stays the highest-precedence binding that keycaps, tooltips,
    /// Alt hints and the settings page display.
    pub fn compatibility_strokes(self) -> &'static [&'static str] {
        match self {
            Self::Filter => &["f3"],
            Self::Terminal => &["f4"],
            // F5 is the historical refresh key; Ctrl+R is the primary one.
            Self::Refresh => &["f5"],
            Self::Sidebar => &["f9"],
            // F10 is the conventional menu key; Alt+M is the primary one.
            Self::Menu => &["f10"],
            _ => &[],
        }
    }
}

impl Command {
    pub fn from_key(event: &anti_webui::prelude::KeyboardEvent) -> Option<Self> {
        Self::ALL.into_iter().find(|command| {
            std::iter::once(command.default_stroke())
                .chain(command.compatibility_strokes().iter().copied())
                .any(|stroke| Self::matches_stroke(stroke, event))
        })
    }
    fn matches_stroke(stroke: &str, event: &anti_webui::prelude::KeyboardEvent) -> bool {
        use anti_webui::prelude::{Key, NamedKey};
        let mut parts = stroke.split('-');
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let Some(key) = parts.find(|part| match *part {
            "ctrl" => {
                ctrl = true;
                false
            }
            "alt" => {
                alt = true;
                false
            }
            "shift" => {
                shift = true;
                false
            }
            _ => true,
        }) else {
            return false;
        };
        if ctrl != event.modifiers.ctrl
            || alt != event.modifiers.alt
            || shift != event.modifiers.shift
            || event.modifiers.logo
        {
            return false;
        }
        match &event.key {
            Key::Character(character) => character.eq_ignore_ascii_case(key),
            Key::Unidentified => false,
            Key::Named(named) => match named {
                NamedKey::Enter => key == "enter",
                NamedKey::Escape => key == "escape",
                NamedKey::Tab => key == "tab",
                NamedKey::Space => key == "space",
                NamedKey::ArrowLeft => key == "left",
                NamedKey::ArrowRight => key == "right",
                NamedKey::ArrowUp => key == "up",
                NamedKey::ArrowDown => key == "down",
                NamedKey::Delete => key == "delete",
                NamedKey::F(number) => {
                    key.strip_prefix('f')
                        .and_then(|number| number.parse::<u8>().ok())
                        == Some(*number)
                }
                _ => false,
            },
        }
    }
    pub fn needs_content_focus(self) -> bool {
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
