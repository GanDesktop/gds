use crate::features::file_operations::commands::{
    Command, ConfigureColumns, FavoriteCurrentFolder,
};

/// The header buttons, the shared action list and the settings list all read
/// one [`Command`]; its key must match the action the root view dispatches.
#[test]
fn column_editor_and_favorite_commands_stay_linked_to_their_actions() {
    assert_eq!(Command::Columns.default_stroke(), "alt-shift-v");
    assert_eq!(Command::Favorite.default_stroke(), "ctrl-d");
    assert!(Command::ALL.contains(&Command::Columns));
    assert!(Command::ALL.contains(&Command::Favorite));
    assert!(
        Command::Columns
            .action()
            .as_ref()
            .partial_eq(&ConfigureColumns)
    );
    assert!(
        Command::Favorite
            .action()
            .as_ref()
            .partial_eq(&FavoriteCurrentFolder)
    );
}
