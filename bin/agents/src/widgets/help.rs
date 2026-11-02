use crate::shared::i18n::tr;
use gds::ui::{
    Shortcut,
    help::{HelpPage, ShortcutHeading, ShortcutRow, ShortcutSection},
};
use gds_uim::el;
use gpui_kit::{App, IntoElement, prelude::*};

pub(crate) fn render(cx: &App) -> gpui_kit::AnyElement {
    let application = vec![
        (
            tr!("app-help"),
            Shortcut::new(Box::new(gds::ui::help::OpenHelp), None),
        ),
        (
            tr!("settings-title"),
            Shortcut::new(Box::new(crate::shared::ui::dialogs::OpenSettings), None),
        ),
        (
            tr!("app-quit"),
            Shortcut::new(Box::new(crate::shared::ui::dialogs::Quit), None),
        ),
    ];
    let settings = vec![
        (
            tr!("settings-return"),
            Shortcut::new(
                Box::new(crate::shared::routes::CloseWindow),
                Some("BasSettings"),
            ),
        ),
        (
            tr!("shortcuts-next-page"),
            Shortcut::new(
                Box::new(crate::shared::routes::NextPage),
                Some("BasSettings"),
            ),
        ),
        (
            tr!("shortcuts-previous-page"),
            Shortcut::new(
                Box::new(crate::shared::routes::PreviousPage),
                Some("BasSettings"),
            ),
        ),
        (
            tr!("sidebar-toggle"),
            Shortcut::new(
                Box::new(crate::shared::routes::ToggleSidebar),
                Some("BasSettings"),
            ),
        ),
    ];
    let notifications = [
        gds::ui::actions::Command::FocusNotifications,
        gds::ui::actions::Command::CloseNotification,
        gds::ui::actions::Command::CloseAllNotifications,
    ]
    .into_iter()
    .map(|command| (gds::i18n::text(command.label_id(), cx), command.into()))
    .collect();
    let dialogs = gds::ui::dialog::DialogCommand::ALL
        .into_iter()
        .map(|command| (gds::i18n::text(command.label_id(), cx), command.shortcut()))
        .collect();
    let sections = [
        (tr!("shortcuts-application"), owned(application)),
        (
            tr!("shortcuts-workspace"),
            owned(crate::features::navigation::commands::shortcut_entries()),
        ),
        (tr!("shortcuts-settings"), owned(settings)),
        (tr!("shortcuts-notifications"), notifications),
        (tr!("shortcuts-dialogs"), dialogs),
    ];
    el! {
        .HelpPage::default [
            ..$(sections.into_iter().filter_map(|(title, entries)| {
                let rows: Vec<_> = entries.into_iter().map(|(label, shortcut)| ShortcutRow::new(label, shortcut)).collect();
                (!rows.is_empty()).then(|| el! {
                    .ShortcutSection::default [
                        .ShortcutHeading::default [title],
                        ..rows,
                    ]
                })
            })),
        ]
    }.into_any_element()
}

fn owned(entries: Vec<(&'static str, Shortcut)>) -> Vec<(gpui_kit::SharedString, Shortcut)> {
    entries
        .into_iter()
        .map(|(label, shortcut)| (label.into(), shortcut))
        .collect()
}
