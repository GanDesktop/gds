use crate::features::file_operations::commands::{
    CloseTerminal, Command, NewTerminal, TERMINAL_TABS_CONTEXT, ordinal_shortcut,
};
use gds::ui::Shortcut;
use gds::{
    i18n::text,
    ui::{
        dialog::DialogCommand,
        help::{HelpPage, ShortcutHeading, ShortcutRow, ShortcutSection},
    },
};
use gds_uim::el;
use gpui_kit::{App, IntoElement, ParentElement, RenderOnce, Window};

const SECTIONS: &[(&str, &[Command])] = &[
    (
        "help-application",
        &[
            Command::Menu,
            Command::Settings,
            Command::Help,
            Command::Cancel,
        ],
    ),
    (
        "help-tabs",
        &[
            Command::NewTab,
            Command::CloseTab,
            Command::NextTab,
            Command::PreviousTab,
        ],
    ),
    (
        "help-navigation",
        &[
            Command::EditLocation,
            Command::Back,
            Command::Forward,
            Command::Parent,
            Command::Refresh,
            Command::Favorite,
        ],
    ),
    (
        "help-view",
        &[
            Command::Sidebar,
            Command::Details,
            Command::ViewMode,
            Command::Columns,
            Command::Sort,
            Command::Filter,
            Command::Hidden,
        ],
    ),
    (
        "help-files",
        &[
            Command::NewFolder,
            Command::Open,
            Command::OpenInNewTab,
            Command::Copy,
            Command::Cut,
            Command::Paste,
            Command::CopyPaths,
            Command::Rename,
            Command::Trash,
            Command::DeletePermanent,
        ],
    ),
    ("help-history", &[Command::Undo, Command::Redo]),
    (
        "help-selection",
        &[
            Command::SelectAll,
            Command::DeselectAll,
            Command::ContextMenu,
        ],
    ),
    (
        "help-locations",
        &[
            Command::MountVolume,
            Command::UnmountVolume,
            Command::ClearRecent,
        ],
    ),
    (
        "help-trash",
        &[
            Command::RestoreSelection,
            Command::PurgeSelection,
            Command::EmptyTrash,
        ],
    ),
    ("help-terminal", &[Command::Terminal]),
    (
        "help-focus",
        &[
            Command::FocusLocations,
            Command::FocusVolumes,
            Command::FocusTabs,
            Command::FocusContent,
        ],
    ),
    (
        "help-notifications",
        &[
            Command::FocusNotifications,
            Command::CloseNotification,
            Command::CloseAllNotifications,
        ],
    ),
];

#[derive(Default, IntoElement)]
pub(crate) struct FilesHelp;

impl RenderOnce for FilesHelp {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        el! {
            .HelpPage::default [
                ..$(SECTIONS.iter().map(|(title, commands)| el! {
                    .ShortcutSection::default [
                        .ShortcutHeading::default [$text(title, cx)],
                        ..$(commands.iter().map(|command| el! {
                            ($(text(command.label_id(), cx)), $*command).ShortcutRow::new
                        })),
                        @if (*title == "help-tabs")
                            ($(text("help-select-tab", cx)), $(ordinal_shortcut(0).expect("first tab binding"))).ShortcutRow::new {
                                range: $(ordinal_shortcut(8).expect("ninth tab binding"));
                            },
                        @end,
                        @if (*title == "help-terminal")
                            ($(text("files-terminal-new", cx)), $(Shortcut::new(Box::new(NewTerminal), Some(TERMINAL_TABS_CONTEXT)))).ShortcutRow::new,
                            ($(text("files-terminal-end", cx)), $(Shortcut::new(Box::new(CloseTerminal), Some(TERMINAL_TABS_CONTEXT)))).ShortcutRow::new,
                        @end,
                    ]
                })),
                .ShortcutSection::default [
                    .ShortcutHeading::default [$text("help-dialogs", cx)],
                    ..$(DialogCommand::ALL.into_iter().map(|command| el! {
                        ($(text(command.label_id(), cx)), $(command.shortcut())).ShortcutRow::new
                    })),
                ],
            ]
        }
    }
}
