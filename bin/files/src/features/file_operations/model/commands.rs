//! Files commands and their workspace-specific default key bindings.

pub use gds::ui::actions::{Cancel, CloseAllNotifications, CloseNotification, FocusNotifications};
use gpui_kit::*;

gpui_kit::actions!(
    gds,
    [
        ApplicationMenu,
        OpenSettings,
        NewFolder,
        NewTab,
        NewWindow,
        CloseTab,
        NextTab,
        PreviousTab,
        ToggleSidebar,
        ToggleTerminal,
        NewTerminal,
        RenameTerminal,
        CloseTerminal,
        NextTerminal,
        PreviousTerminal,
        FocusTerminal,
        ReturnFromTerminal,
        EditLocation,
        NavigateBack,
        NavigateForward,
        NavigateParent,
        Refresh,
        FilterNames,
        ToggleHidden,
        OpenSelection,
        OpenSelectionInNewTabs,
        CopyFiles,
        CutFiles,
        PasteFiles,
        UndoFileOperation,
        RedoFileOperation,
        CopyFilePaths,
        SelectAllFiles,
        DeselectAll,
        SelectionMenu,
        RenameSelection,
        ToggleDetails,
        TrashSelection,
        DeleteSelectionPermanently,
        ConfigureColumns,
        ToggleViewMode,
        SortFiles,
        FavoriteCurrentFolder,
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
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectTab9,
    ]
);

pub const WORKSPACE_CONTEXT: &str = "Workspace";
pub const FILE_TABLE_CONTEXT: &str = "FilesFileTable";
/// Files owns multi-terminal navigation; it is distinct from PTY input.
pub const TERMINAL_TABS_CONTEXT: &str = "FilesTerminalTabs";
/// The whole terminal half, including its header controls.
pub const TERMINAL_CONTEXT: &str = "FilesTerminal";
/// Key context of the settings workspace, active while settings replace the
/// file workspace. Bindings here keep shortcuts from leaking between the two.
pub const SETTINGS_CONTEXT: &str = "Settings";
/// Operations on the selected volume or the visible fixed volume page.
pub const VOLUME_CONTEXT: &str = "Volume";

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
    pub const ALL: [Self; 50] = [
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

    pub fn action(self) -> Box<dyn Action> {
        match self {
            Self::Menu => Box::new(ApplicationMenu),
            Self::Settings => Box::new(OpenSettings),
            Self::Help => Box::new(gds::ui::help::OpenHelp),
            Self::NewFolder => Box::new(NewFolder),
            Self::NewTab => Box::new(NewTab),
            Self::NewWindow => Box::new(NewWindow),
            Self::CloseTab => Box::new(CloseTab),
            Self::NextTab => Box::new(NextTab),
            Self::PreviousTab => Box::new(PreviousTab),
            Self::Sidebar => Box::new(ToggleSidebar),
            Self::Terminal => Box::new(ToggleTerminal),
            Self::EditLocation => Box::new(EditLocation),
            Self::Back => Box::new(NavigateBack),
            Self::Forward => Box::new(NavigateForward),
            Self::Parent => Box::new(NavigateParent),
            Self::Refresh => Box::new(Refresh),
            Self::Filter => Box::new(FilterNames),
            Self::Hidden => Box::new(ToggleHidden),
            Self::Favorite => Box::new(FavoriteCurrentFolder),
            Self::Open => Box::new(OpenSelection),
            Self::OpenInNewTab => Box::new(OpenSelectionInNewTabs),
            Self::Copy => Box::new(CopyFiles),
            Self::Cut => Box::new(CutFiles),
            Self::Paste => Box::new(PasteFiles),
            Self::Undo => Box::new(UndoFileOperation),
            Self::Redo => Box::new(RedoFileOperation),
            Self::CopyPaths => Box::new(CopyFilePaths),
            Self::SelectAll => Box::new(SelectAllFiles),
            Self::DeselectAll => Box::new(DeselectAll),
            Self::ContextMenu => Box::new(SelectionMenu),
            Self::Rename => Box::new(RenameSelection),
            Self::Details => Box::new(ToggleDetails),
            Self::Columns => Box::new(ConfigureColumns),
            Self::ViewMode => Box::new(ToggleViewMode),
            Self::Sort => Box::new(SortFiles),
            Self::Trash => Box::new(TrashSelection),
            Self::DeletePermanent => Box::new(DeleteSelectionPermanently),
            Self::RestoreSelection => Box::new(RestoreSelection),
            Self::PurgeSelection => Box::new(PurgeSelection),
            Self::EmptyTrash => Box::new(EmptyTrash),
            Self::ClearRecent => Box::new(ClearRecent),
            Self::MountVolume => Box::new(MountVolume),
            Self::UnmountVolume => Box::new(UnmountVolume),
            Self::FocusLocations => Box::new(FocusLocations),
            Self::FocusVolumes => Box::new(FocusVolumes),
            Self::FocusTabs => Box::new(FocusTabs),
            Self::FocusContent => Box::new(FocusContent),
            Self::FocusNotifications => Box::new(FocusNotifications),
            Self::CloseNotification => Box::new(CloseNotification),
            Self::CloseAllNotifications => Box::new(CloseAllNotifications),
            Self::Cancel => Box::new(Cancel),
        }
    }

    pub fn context(self) -> Option<&'static str> {
        match self {
            Self::Settings
            | Self::Help
            | Self::Terminal
            | Self::Sidebar
            | Self::Cancel
            | Self::FocusNotifications
            | Self::CloseNotification
            | Self::CloseAllNotifications => None,
            Self::MountVolume | Self::UnmountVolume => Some(VOLUME_CONTEXT),
            _ => Some(WORKSPACE_CONTEXT),
        }
    }

    pub fn binding(self, window: &Window) -> Option<KeyBinding> {
        let action = self.action();
        let context = self
            .context()
            .map(|context| KeyContext::parse(context).expect("bundled action contexts are valid"))
            .unwrap_or_default();
        window.highest_precedence_binding_for_action_in_context(action.as_ref(), context)
    }

    /// The default keystroke for this command, as registered by [`register`].
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
    /// [`register`] adds them before [`Self::default_stroke`], so the primary
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

pub fn register(cx: &mut App) {
    let bindings = bindings(cx);
    cx.bind_keys(bindings);
    gds::ui::dialog::register(cx);
}

/// Every bundled binding, in precedence order: compatibility aliases first,
/// then primary strokes. The last binding for an action is the one displayed.
fn bindings(cx: &App) -> Vec<KeyBinding> {
    let compatibility = Command::ALL.into_iter().flat_map(|command| {
        command
            .compatibility_strokes()
            .iter()
            .copied()
            .map(move |stroke| binding(stroke, command, command.context(), cx))
    });
    let primary = Command::ALL
        .into_iter()
        .chain([Command::Cancel])
        .map(|command| binding(command.default_stroke(), command, command.context(), cx));
    // The same cycle keys navigate settings pages in their own context.
    let settings = [
        (Command::NextTab, "ctrl-tab"),
        (Command::PreviousTab, "ctrl-shift-tab"),
    ]
    .into_iter()
    .map(|(command, stroke)| binding(stroke, command, Some(SETTINGS_CONTEXT), cx));
    compatibility
        .chain(primary)
        .chain(settings)
        .chain(["Settings", "Help"].into_iter().flat_map(|context| {
            ["alt-left", "ctrl-w"]
                .into_iter()
                .map(move |stroke| binding(stroke, Command::Cancel, Some(context), cx))
        }))
        .chain(ordinal_bindings(cx))
        .chain(terminal_bindings())
        .chain(file_table_bindings())
        // Popup cancellation must win over the workspace's global Escape.
        // Otherwise it moves focus behind a menu while leaving that menu open.
        .chain([
            action_binding(
                "escape",
                Box::new(gpui_kit::base::actions::Cancel),
                Some("PopupMenu"),
                cx,
            ),
            action_binding(
                "enter",
                Box::new(gpui_kit::base::actions::Confirm { secondary: false }),
                Some("PopupMenu"),
                cx,
            ),
        ])
        .collect()
}

/// Files owns file selection; remove only the native table actions so raw
/// navigation reaches Browser and Tab retains Root's focus traversal.
fn file_table_bindings() -> [KeyBinding; 11] {
    use gpui_kit::base::actions::{
        Cancel, SelectDown, SelectFirst, SelectLast, SelectNextColumn, SelectPageDown,
        SelectPageUp, SelectPrevColumn, SelectUp,
    };
    fn unbind(stroke: &'static str, action: impl Action) -> KeyBinding {
        KeyBinding::new(
            stroke,
            Unbind(action.name().into()),
            Some("(FilesFileTable > DataTable) && !PopupMenu"),
        )
    }
    [
        unbind("tab", SelectNextColumn),
        unbind("shift-tab", SelectPrevColumn),
        unbind("up", SelectUp),
        unbind("down", SelectDown),
        unbind("left", SelectPrevColumn),
        unbind("right", SelectNextColumn),
        unbind("home", SelectFirst),
        unbind("end", SelectLast),
        unbind("pageup", SelectPageUp),
        unbind("pagedown", SelectPageDown),
        unbind("escape", Cancel),
    ]
}

fn ordinal_bindings(cx: &App) -> Vec<KeyBinding> {
    let context = Some(WORKSPACE_CONTEXT);
    vec![
        action_binding("ctrl-1", Box::new(SelectTab1), context, cx),
        action_binding("ctrl-2", Box::new(SelectTab2), context, cx),
        action_binding("ctrl-3", Box::new(SelectTab3), context, cx),
        action_binding("ctrl-4", Box::new(SelectTab4), context, cx),
        action_binding("ctrl-5", Box::new(SelectTab5), context, cx),
        action_binding("ctrl-6", Box::new(SelectTab6), context, cx),
        action_binding("ctrl-7", Box::new(SelectTab7), context, cx),
        action_binding("ctrl-8", Box::new(SelectTab8), context, cx),
        action_binding("ctrl-9", Box::new(SelectTab9), context, cx),
    ]
}

/// Terminal capabilities are shared, but their key choices belong to Files.
fn terminal_bindings() -> Vec<KeyBinding> {
    use gds::ui::terminal::{
        Copy, INPUT_CONTEXT, Paste, ScrollBottom, ScrollPageDown, ScrollPageUp, ScrollTop,
    };
    let whole_terminal = Some("GdsTerminal || FilesTerminalTabs || FilesTerminal");
    vec![
        KeyBinding::new("ctrl-n", NewTerminal, whole_terminal),
        KeyBinding::new("ctrl-w", CloseTerminal, whole_terminal),
        KeyBinding::new("f6", ReturnFromTerminal, whole_terminal),
        KeyBinding::new("ctrl-shift-c", Copy, Some(INPUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-v", Paste, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-pageup", ScrollPageUp, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-pagedown", ScrollPageDown, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-home", ScrollTop, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-end", ScrollBottom, Some(INPUT_CONTEXT)),
        KeyBinding::new("f2", RenameTerminal, Some(INPUT_CONTEXT)),
        KeyBinding::new("f2", RenameTerminal, Some(TERMINAL_TABS_CONTEXT)),
        KeyBinding::new("down", NextTerminal, Some(TERMINAL_TABS_CONTEXT)),
        KeyBinding::new("up", PreviousTerminal, Some(TERMINAL_TABS_CONTEXT)),
        KeyBinding::new("enter", FocusTerminal, Some(TERMINAL_TABS_CONTEXT)),
    ]
}

fn binding(
    stroke: &'static str,
    command: Command,
    context: Option<&'static str>,
    cx: &App,
) -> KeyBinding {
    action_binding(stroke, command.action(), context, cx)
}

fn action_binding(
    stroke: &'static str,
    action: Box<dyn Action>,
    context: Option<&'static str>,
    cx: &App,
) -> KeyBinding {
    // Context predicates see the whole focus ancestry. Both terminal regions
    // exclude Files bindings; the panel's own tab-bar keys must not become
    // file-tab operations, and PTY input must keep its control characters.
    let scoped = if action.partial_eq(&gds::ui::help::OpenHelp)
        || action.partial_eq(&OpenSettings)
        // Only the primary terminal toggle is window-wide; PTY input keeps F4.
        || (action.partial_eq(&ToggleTerminal) && stroke == Command::Terminal.default_stroke())
        || action.partial_eq(&FocusNotifications)
        || action.partial_eq(&CloseNotification)
        || action.partial_eq(&CloseAllNotifications)
        || context == Some("PopupMenu")
    {
        context.map(str::to_owned)
    } else {
        Some(match context {
            Some(context) => {
                format!("({context}) && !GdsTerminal && !FilesTerminalTabs && !FilesTerminal")
            }
            None => "!GdsTerminal && !FilesTerminalTabs && !FilesTerminal".to_owned(),
        })
    };
    let scoped = if action.partial_eq(&UndoFileOperation) || action.partial_eq(&RedoFileOperation) {
        scoped.map(|context| format!("({context}) && !Input"))
    } else {
        scoped
    };
    let predicate = scoped.as_deref().map(|value| {
        KeyBindingContextPredicate::parse(value)
            .expect("bundled action contexts are valid")
            .into()
    });
    KeyBinding::load(stroke, action, predicate, false, None, cx)
        .expect("bundled action shortcuts are valid")
}

#[cfg(test)]
mod tests;

impl From<Command> for gds::ui::Shortcut {
    fn from(command: Command) -> Self {
        Self::new(command.action(), command.context())
    }
}

/// Direct binding for the `position + 1`-th file tab.
///
/// The strip is the cycle order, pinned tabs included; rows past the ninth have
/// no key and therefore no hint.
pub fn ordinal_shortcut(position: usize) -> Option<gds::ui::Shortcut> {
    let action: Box<dyn Action> = match position {
        0 => Box::new(SelectTab1),
        1 => Box::new(SelectTab2),
        2 => Box::new(SelectTab3),
        3 => Box::new(SelectTab4),
        4 => Box::new(SelectTab5),
        5 => Box::new(SelectTab6),
        6 => Box::new(SelectTab7),
        7 => Box::new(SelectTab8),
        8 => Box::new(SelectTab9),
        _ => return None,
    };
    Some(gds::ui::Shortcut::new(action, Some(WORKSPACE_CONTEXT)))
}
