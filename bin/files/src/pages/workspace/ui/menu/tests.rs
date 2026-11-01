use crate::features::file_operations::commands::{ApplicationMenu, Command};

use super::MenuEntry;

/// Only settings is tied to a workspace command.
#[test]
fn menu_rows_offer_settings_logs_and_about() {
    assert_eq!(
        MenuEntry::ROWS,
        [MenuEntry::Settings, MenuEntry::Logs, MenuEntry::About]
    );
    assert_eq!(MenuEntry::Settings.command(), Some(Command::Settings));
    assert_eq!(MenuEntry::About.command(), None);
    assert!(
        Command::ALL.contains(&Command::Settings),
        "settings is also listed in the settings page"
    );
    assert_eq!(MenuEntry::Settings.label_id(), Command::Settings.label_id());
    assert_eq!(MenuEntry::About.label_id(), "about");
}

/// The menu a press opens is the one the toolbar shows: one command, one
/// action, and `Alt+M` as its displayed key while `F10` stays an alias.
#[test]
fn menu_command_is_the_action_the_toolbar_opens() {
    assert_eq!(Command::Menu.label_id(), "app-menu");
    assert!(Command::ALL.contains(&Command::Menu));
    assert!(Command::Menu.action().as_ref().partial_eq(&ApplicationMenu));
    assert_eq!(Command::Menu.default_stroke(), "alt-m");
    assert_eq!(Command::Menu.compatibility_strokes(), &["f10"][..]);
}
