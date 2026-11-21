use zgui::vocab::{Key, KeyEvent, Modifiers};

use crate::ui::shortcut::Shortcut;

/// Keyboard confirmation/cancellation commands routed to active modal dialogs (`shortcuts.md` §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogCommand {
    Confirm,
    Cancel,
}

impl DialogCommand {
    /// Every dialog command in canonical order.
    pub const ALL: [Self; 2] = [Self::Confirm, Self::Cancel];

    /// Fluent localization message ID describing this dialog command.
    #[must_use]
    pub const fn label_id(self) -> &'static str {
        match self {
            Self::Confirm => "dialog-confirm",
            Self::Cancel => "dialog-cancel",
        }
    }

    /// Default mnemonic [`Shortcut`] bound to this dialog command (`Alt+A` / `Alt+C`).
    #[must_use]
    pub fn default_stroke(self) -> Shortcut {
        match self {
            Self::Confirm => Shortcut::new(Key::character("a"), Modifiers::ALT),
            Self::Cancel => Shortcut::new(Key::character("c"), Modifiers::ALT),
        }
    }

    /// Alias for [`Self::default_stroke`].
    #[must_use]
    pub fn shortcut(self) -> Shortcut {
        self.default_stroke()
    }

    /// Returns `true` when `key` and `modifiers` match this command's default stroke.
    #[must_use]
    pub fn matches(self, key: &Key, modifiers: Modifiers) -> bool {
        self.default_stroke().matches(key, modifiers)
    }

    /// Returns `true` when a key event and `modifiers` match this command's default stroke.
    #[must_use]
    pub fn matches_event(self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        self.default_stroke().matches_event(event, modifiers)
    }
}

impl From<DialogCommand> for Shortcut {
    fn from(command: DialogCommand) -> Self {
        command.default_stroke()
    }
}
