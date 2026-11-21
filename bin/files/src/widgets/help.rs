//! Keyboard reference assembled from the same command metadata as the workspace.

use gds::i18n;
use gds::ui::Shortcut;
use gds::ui::dialog::DialogCommand;
use gds::ui::help::{HelpPageProps, ShortcutHeadingProps, ShortcutRowProps, ShortcutSectionProps};
use zgui::prelude::*;
use zgui::vocab::{Key, Modifiers};
use zgui::{component, view};

use crate::shared::ui::actions::{Command, ordinal_shortcut};

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

/// Renders the complete keyboard shortcut reference page for GDS Files.
#[component]
pub(crate) fn FilesHelp() -> impl IntoView {
    let section_views = SECTIONS
        .iter()
        .map(|&(title, commands)| {
            let rows = commands
                .iter()
                .map(|&command| {
                    view! {
                        ShortcutRow(
                            label = Signal::derive_local(move || i18n::text(command.label_id())),
                            shortcut = command.shortcut(),
                        )
                    }
                    .into_view()
                })
                .collect::<Vec<_>>();
            let extra_tabs = (title == "help-tabs").then(|| {
                let first_tab = ordinal_shortcut(0).expect("first tab binding");
                let last_tab = ordinal_shortcut(8).expect("ninth tab binding");
                view! {
                    ShortcutRow(
                        label = Signal::derive_local(|| i18n::text("help-select-tab")),
                        shortcut = first_tab,
                        range_end = last_tab,
                    )
                }
                .into_view()
            });
            let extra_terminal = (title == "help-terminal").then(|| {
                vec![
                    view! {
                        ShortcutRow(
                            label = Signal::derive_local(|| i18n::text("files-terminal-new")),
                            shortcut = Shortcut::new(Key::character("n"), Modifiers::CONTROL),
                        )
                    }
                    .into_view(),
                    view! {
                        ShortcutRow(
                            label = Signal::derive_local(|| i18n::text("files-terminal-end")),
                            shortcut = Shortcut::new(Key::character("w"), Modifiers::CONTROL),
                        )
                    }
                    .into_view(),
                ]
            });
            view! {
                ShortcutSection {
                    ShortcutHeading {{move || i18n::text(title)}}
                    {rows}
                    {extra_tabs}
                    {extra_terminal}
                }
            }
            .into_view()
        })
        .collect::<Vec<_>>();

    let dialog_rows = DialogCommand::ALL
        .into_iter()
        .map(|command| {
            view! {
                ShortcutRow(
                    label = Signal::derive_local(move || i18n::text(command.label_id())),
                    shortcut = command.shortcut(),
                )
            }
            .into_view()
        })
        .collect::<Vec<_>>();

    view! {
        HelpPage {
            {section_views}
            ShortcutSection {
                ShortcutHeading {{move || i18n::text("help-dialogs")}}
                {dialog_rows}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use gds::ui::dialog::DialogCommand;
    use zgui::reactive::{Mounted, install};
    use zgui::vocab::{Key, Modifiers};

    use super::{FilesHelpProps, SECTIONS};
    use crate::shared::ui::actions::Command;

    #[test]
    fn files_help_renders_all_sections_and_uses_dialog_command_mnemonics() {
        assert_eq!(SECTIONS.len(), 12);
        assert_eq!(
            DialogCommand::ALL.map(|cmd| (cmd.label_id(), cmd.shortcut())),
            [
                (
                    "dialog-confirm",
                    gds::ui::Shortcut::new(Key::character("a"), Modifiers::ALT),
                ),
                (
                    "dialog-cancel",
                    gds::ui::Shortcut::new(Key::character("c"), Modifiers::ALT),
                ),
            ]
        );
        assert!(
            SECTIONS
                .iter()
                .any(|&(title, cmds)| title == "help-notifications"
                    && cmds
                        == [
                            Command::FocusNotifications,
                            Command::CloseNotification,
                            Command::CloseAllNotifications,
                        ])
        );

        let _ = install();
        let scope = Mounted::new();
        scope.with(|| {
            let _gds = gds::app::Gds::install(gds::service::task_manager::Scope::default());
            let _view = zgui::view! { FilesHelp() };
        });
        scope.unmount();
    }
}
