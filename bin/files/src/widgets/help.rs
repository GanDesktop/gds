//! Keyboard reference assembled from the same command metadata as the workspace.
use crate::shared::ui::actions::Command;
use anti_webui::prelude::*;
use gds::{
    i18n,
    ui::dialog::DialogCommand,
    ui::help::{HelpPage, ShortcutHeading, ShortcutRow, ShortcutSection},
};
use gds_uim::el;
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

#[derive(Default)]
pub(crate) struct FilesHelp;
impl IntoView for FilesHelp {
    fn into_view(self) -> View {
        el! { .HelpPage::default.[
            (move () => ${
                el! { .div.[..SECTIONS.{iter;map:$(|(title, commands)| {
                    let rows=commands.iter().map(|command| ShortcutRow::new(i18n::text(command.label_id()),command.default_stroke()));
                    el! { .ShortcutSection::default.[
                        .ShortcutHeading::default.[(*title).i18n::text.text],
                        ..rows,
                        if $(*title=="help-tabs") [$(ShortcutRow::new(i18n::text("help-select-tab"),"Ctrl+1 – Ctrl+9"))],
                        if $(*title=="help-terminal") [
                            $(ShortcutRow::new(i18n::text("files-terminal-new"),"Ctrl+N")),
                            $(ShortcutRow::new(i18n::text("files-terminal-end"),"Ctrl+W")),
                        ],
                    ] }
                })},
                    .ShortcutSection::default.[
                        .ShortcutHeading::default.["help-dialogs".i18n::text.text],
                        ..$(DialogCommand::ALL.into_iter().map(|command| {
                            ShortcutRow::new(i18n::text(command.label_id()), command.default_stroke())
                        })),
                    ],
                ] }
            }).dyn_view,
        ] }
        .into_view()
    }
}
