use crate::features::file_operations::commands::{
    CloseTerminal, Command, NewTerminal, TERMINAL_TABS_CONTEXT, ordinal_shortcut,
};
use gds::{
    i18n::text,
    ui::{
        dialog::DialogCommand,
        help::{HelpPage, ShortcutHeading, ShortcutRow, ShortcutSection},
    },
};
use gds_uim::el;
use gpui_kit::{App, ParentElement};

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

pub(crate) fn render(cx: &App) -> HelpPage {
    el! {
        .HelpPage::default [
            ..$(SECTIONS.iter().map(|(title, commands)| el! {
                .ShortcutSection::default [
                    .ShortcutHeading::default [$text(title, cx)],
                    ..$(commands.iter().map(|command| ShortcutRow::new(text(command.label_id(), cx), *command))),
                    @if (*title == "help-tabs")
                        $(ShortcutRow::new(text("help-select-tab", cx), ordinal_shortcut(0).expect("first tab binding"))
                            .range(ordinal_shortcut(8).expect("ninth tab binding"))),
                    @end,
                    @if (*title == "help-terminal")
                        $(ShortcutRow::new(text("files-terminal-new", cx), gds::ui::Shortcut::new(Box::new(NewTerminal), Some(TERMINAL_TABS_CONTEXT)))),
                        $(ShortcutRow::new(text("files-terminal-end", cx), gds::ui::Shortcut::new(Box::new(CloseTerminal), Some(TERMINAL_TABS_CONTEXT)))),
                    @end,
                ]
            })),
            .ShortcutSection::default [
                .ShortcutHeading::default [$text("help-dialogs", cx)],
                ..$(DialogCommand::ALL.into_iter().map(|command| ShortcutRow::new(text(command.label_id(), cx), command.shortcut()))),
            ],
        ]
    }
}
