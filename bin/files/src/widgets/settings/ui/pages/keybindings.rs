use gds::ui::settings::*;
use gds::{
    i18n::text,
    ui::{ShortcutBadge, dialog::DialogCommand},
};
use gds_uim::el;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use crate::features::file_operations::commands::Command;
use crate::widgets::settings::ui::{View, controls::SettingNote};

/// Trash actions are bound globally but only run on the Trash page; listing
/// them apart keeps every row's key honest.
const TRASH_COMMANDS: [Command; 3] = [
    Command::RestoreSelection,
    Command::PurgeSelection,
    Command::EmptyTrash,
];
/// Actions that belong to one location page rather than the file workspace.
const PAGE_COMMANDS: [Command; 3] = [
    Command::ClearRecent,
    Command::MountVolume,
    Command::UnmountVolume,
];

const REGION_COMMANDS: [Command; 4] = [
    Command::FocusLocations,
    Command::FocusVolumes,
    Command::FocusTabs,
    Command::FocusContent,
];
const NOTIFICATION_COMMANDS: [Command; 3] = [
    Command::FocusNotifications,
    Command::CloseNotification,
    Command::CloseAllNotifications,
];

impl View {
    pub(super) fn render_keybindings(&self, cx: &App) -> Div {
        let note = SettingNote::new(text("settings-shortcuts-note", cx));
        let places_note = SettingNote::new(text("settings-places-shortcuts", cx));
        let prefix = SettingNote::new(text("settings-tab-prefix", cx));
        let rows: Vec<_> = Command::ALL
            .into_iter()
            .filter(|command| {
                !TRASH_COMMANDS.contains(command)
                    && !PAGE_COMMANDS.contains(command)
                    && !REGION_COMMANDS.contains(command)
                    && !NOTIFICATION_COMMANDS.contains(command)
            })
            .map(|command| {
                el! {
                    .SettingRow::default [
                        .SettingLabel::default [$text(command.label_id(), cx)],
                        .SettingActions::default [
                            .SettingReset::default,
                            .SettingControl::default [command.ShortcutBadge::new],
                        ],
                    ]
                }
            })
            .collect();
        let page_heading = section_heading("settings-page-actions", cx);
        let region_heading = section_heading("settings-region-actions", cx);
        let region_note = SettingNote::new(text("settings-region-note", cx));
        let region_rows: Vec<_> = REGION_COMMANDS
            .into_iter()
            .map(|command| {
                el! {
                    .SettingRow::default [
                        .SettingLabel::default [$text(command.label_id(), cx)],
                        .SettingActions::default [
                            .SettingReset::default,
                            .SettingControl::default [command.ShortcutBadge::new],
                        ],
                    ]
                }
            })
            .collect();
        let notification_heading = section_heading("settings-notification-actions", cx);
        let notification_note = SettingNote::new(text("settings-notification-note", cx));
        let notification_rows: Vec<_> = NOTIFICATION_COMMANDS
            .into_iter()
            .map(|command| {
                el! {
                    .SettingRow::default [
                        .SettingLabel::default [$text(command.label_id(), cx)],
                        .SettingActions::default [
                            .SettingReset::default,
                            .SettingControl::default [command.ShortcutBadge::new],
                        ],
                    ]
                }
            })
            .collect();
        let page_rows: Vec<_> = PAGE_COMMANDS
            .into_iter()
            .map(|command| {
                el! {
                    .SettingRow::default [
                        .SettingLabel::default [$text(command.label_id(), cx)],
                        .SettingActions::default [
                            .SettingReset::default,
                            .SettingControl::default [command.ShortcutBadge::new],
                        ],
                    ]
                }
            })
            .collect();
        let trash_heading = section_heading("settings-trash-actions", cx);
        let trash_note = SettingNote::new(text("settings-trash-note", cx));
        let trash_rows: Vec<_> = TRASH_COMMANDS
            .into_iter()
            .map(|command| {
                el! {
                    .SettingRow::default [
                        .SettingLabel::default [$text(command.label_id(), cx)],
                        .SettingActions::default [
                            .SettingReset::default,
                            .SettingControl::default [command.ShortcutBadge::new],
                        ],
                    ]
                }
            })
            .collect();
        let dialog_heading_label = text("settings-dialogs", cx);
        let dialog_heading = el! {
            .div { pt_6; pb_2; text_xs; text_color: $(cx.theme().muted_foreground) }
            [dialog_heading_label]
        };
        let dialog_note = SettingNote::new(text("settings-dialogs-note", cx));
        let dialog_rows: Vec<_> = DialogCommand::ALL
            .into_iter()
            .map(|command| {
                el! {
                    .SettingRow::default [
                        .SettingLabel::default [$text(command.label_id(), cx)],
                        .SettingActions::default [
                            .SettingReset::default,
                            .SettingControl::default [$(command.shortcut()).ShortcutBadge::bound],
                        ],
                    ]
                }
            })
            .collect();
        el! {
            .div [
                note, places_note, ..rows,
                prefix,
                region_heading, region_note, ..region_rows,
                notification_heading, notification_note, ..notification_rows,
                page_heading, ..page_rows,
                trash_heading, trash_note, ..trash_rows,
                dialog_heading, dialog_note, ..dialog_rows,
            ]
        }
    }
}

fn section_heading(id: &'static str, cx: &App) -> Div {
    let label = text(id, cx);
    el! {
        .div { pt_6; pb_2; text_xs; text_color: $(cx.theme().muted_foreground) }
        [label]
    }
}
