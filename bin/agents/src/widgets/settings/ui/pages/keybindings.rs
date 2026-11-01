use crate::shared::i18n::tr;
use gds::ui::{Shortcut, ShortcutBadge, settings::*};
use gds_uim::el;
use gpui_kit::{App, IntoElement, TestSupportExt, component::ActiveTheme, div, prelude::*};

pub(in crate::widgets::settings) fn render(query: &str, cx: &App) -> gpui_kit::AnyElement {
    let application = vec![
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
                Box::new(super::super::window::CloseWindow),
                Some("BasSettings"),
            ),
        ),
        (
            tr!("shortcuts-next-page"),
            Shortcut::new(
                Box::new(super::super::window::NextPage),
                Some("BasSettings"),
            ),
        ),
        (
            tr!("shortcuts-previous-page"),
            Shortcut::new(
                Box::new(super::super::window::PreviousPage),
                Some("BasSettings"),
            ),
        ),
        (
            tr!("sidebar-toggle"),
            Shortcut::new(
                Box::new(super::super::window::ToggleSidebar),
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
        (tr!("settings-title"), owned(application)),
        (
            tr!("shortcuts-workspace"),
            owned(crate::features::navigation::commands::settings_entries()),
        ),
        (tr!("shortcuts-settings"), owned(settings)),
        (tr!("shortcuts-notifications"), notifications),
        (tr!("shortcuts-dialogs"), dialogs),
    ];
    let query = query.trim().to_lowercase();
    let query = if ["shortcuts", "keyboard shortcuts", "keybindings", "快捷键"]
        .contains(&query.as_str())
    {
        String::new()
    } else {
        query
    };
    el! {
        .div { # "settings-keybindings-page"; test_support; size_full; min_h_0; overflow_y_scroll; p_6 } [
            .div { pb_4; text_sm; text_color: $(cx.theme().muted_foreground) } [tr!("shortcuts-note")],
            ..$(sections.into_iter().filter_map(|(title, entries)| {
                let rows: Vec<_> = entries.into_iter().filter(|(label, _)| {
                    query.is_empty() || title.to_lowercase().contains(&query) || label.to_lowercase().contains(&query)
                }).map(|(label, shortcut)| el! {
                    .SettingRow::default [
                        .SettingLabel::default [label],
                        .SettingActions::default [
                            .SettingReset::default,
                            .SettingControl::default [shortcut.ShortcutBadge::bound],
                        ],
                    ]
                }).collect();
                (!rows.is_empty()).then(|| el! {
                    .div { pb_6 } [
                        .div { pb_2; text_xs; text_color: $(cx.theme().muted_foreground) } [title],
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
