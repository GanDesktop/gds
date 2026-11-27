//! Shared command identities, keystroke defaults, and guarded keyboard dispatch.

use zgui::view::focus_is_text_entry;
use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey};

use super::shortcut::Shortcut;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Cancel;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FocusNotifications;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CloseNotification;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CloseAllNotifications;

/// Core semantic UI commands shared across windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    FocusNotifications,
    CloseNotification,
    CloseAllNotifications,
    Cancel,
}

impl Command {
    /// Every core [`Command`] in canonical menu/help presentation order.
    pub const ALL: [Self; 4] = [
        Self::FocusNotifications,
        Self::CloseNotification,
        Self::CloseAllNotifications,
        Self::Cancel,
    ];

    /// Fluent localization message ID describing this command.
    #[must_use]
    pub const fn label_id(self) -> &'static str {
        match self {
            Self::FocusNotifications => "focus-notifications",
            Self::CloseNotification => "notification-close",
            Self::CloseAllNotifications => "notifications-close-all",
            Self::Cancel => "close",
        }
    }

    /// Default platform-aware [`Shortcut`] bound to this command.
    #[must_use]
    pub fn default_stroke(self) -> Shortcut {
        match self {
            Self::FocusNotifications => Shortcut::new(Key::character("n"), Modifiers::ALT),
            Self::CloseNotification => Shortcut::new(Key::character("x"), Modifiers::ALT),
            Self::CloseAllNotifications => {
                Shortcut::new(Key::character("x"), Modifiers::ALT | Modifiers::SHIFT)
            }
            Self::Cancel => Shortcut::new(Key::Named(NamedKey::Escape), Modifiers::NONE),
        }
    }

    /// Returns `true` when `key` and `modifiers` match this command's default binding.
    #[must_use]
    pub fn matches(self, key: &Key, modifiers: Modifiers) -> bool {
        self.default_stroke().matches(key, modifiers)
    }

    /// Returns `true` when `event` and `modifiers` match this command's default binding.
    #[must_use]
    pub fn matches_event(self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        self.default_stroke().matches_event(event, modifiers)
    }
}

impl Shortcut {
    /// Returns the default shortcut bound to `command`.
    #[must_use]
    pub fn command(command: Command) -> Self {
        command.default_stroke()
    }
}

impl From<Command> for Shortcut {
    fn from(command: Command) -> Self {
        command.default_stroke()
    }
}

/// Format a localized label with a command's default shortcut.
#[must_use]
pub fn command_tooltip(label: &str, command: Command) -> String {
    command.default_stroke().hint(label)
}

/// Ordered keyboard command matcher that honors `!focus_is_text_entry()` for unmodified and
/// `Shift`-only shortcuts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionDispatcher<C = Command> {
    bindings: Vec<(Shortcut, C)>,
    allow_in_text_entry: bool,
}

impl<C: Clone + 'static> Default for ActionDispatcher<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionDispatcher<Command> {
    /// Creates a dispatcher populated with all core [`Command`] bindings.
    #[must_use]
    pub fn commands() -> Self {
        let mut dispatcher = Self::new();
        for command in Command::ALL {
            dispatcher.push(command.default_stroke(), command);
        }
        dispatcher
    }
}

impl<C: Clone + 'static> ActionDispatcher<C> {
    /// Creates an empty dispatcher.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
            allow_in_text_entry: false,
        }
    }

    /// Appends a `(shortcut, command)` binding in builder style.
    #[must_use]
    pub fn with_binding(mut self, shortcut: Shortcut, command: C) -> Self {
        self.bindings.push((shortcut, command));
        self
    }

    /// Appends a `(shortcut, command)` binding.
    pub fn push(&mut self, shortcut: Shortcut, command: C) {
        self.bindings.push((shortcut, command));
    }

    /// Controls whether unmodified / `Shift`-only bindings still match while a text input holds
    /// focus. Defaults to `false`.
    #[must_use]
    pub fn allow_text_entry(mut self, allow: bool) -> Self {
        self.allow_in_text_entry = allow;
        self
    }

    /// Returns the first command matching `event` and `modifiers` while guarding unmodified /
    /// `Shift`-only bindings with `!focus_is_text_entry()`.
    #[must_use]
    pub fn match_event(&self, event: &KeyEvent, modifiers: Modifiers) -> Option<C> {
        self.match_event_with_guard(event, modifiers, focus_is_text_entry())
    }

    /// Pure matching variant accepting an explicit `text_entry_focused` state for callers/tests.
    #[must_use]
    pub fn match_event_with_guard(
        &self,
        event: &KeyEvent,
        modifiers: Modifiers,
        text_entry_focused: bool,
    ) -> Option<C> {
        for (shortcut, command) in &self.bindings {
            if shortcut.requires_non_text_focus() && text_entry_focused && !self.allow_in_text_entry
            {
                continue;
            }
            if shortcut.matches_event(event, modifiers) {
                return Some(command.clone());
            }
        }
        None
    }

    /// Returns the first command matching `(key, modifiers)` using the window text-entry guard.
    #[must_use]
    pub fn match_key(&self, key: &Key, modifiers: Modifiers) -> Option<C> {
        let text_entry_focused = focus_is_text_entry();
        for (shortcut, command) in &self.bindings {
            if shortcut.requires_non_text_focus() && text_entry_focused && !self.allow_in_text_entry
            {
                continue;
            }
            if shortcut.matches(key, modifiers) {
                return Some(command.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use zgui::vocab::{KeyEvent, Modifiers, NamedKey, PhysicalKey};

    use super::{ActionDispatcher, Command};
    use crate::ui::shortcut::Shortcut;

    #[test]
    fn core_command_labels_and_notification_shortcuts_match_spec() {
        assert_eq!(Command::Cancel.label_id(), "close");
        assert_eq!(
            Command::FocusNotifications.label_id(),
            "focus-notifications"
        );
        assert_eq!(Command::CloseNotification.label_id(), "notification-close");
        assert_eq!(
            Command::CloseAllNotifications.label_id(),
            "notifications-close-all"
        );

        let dispatcher = ActionDispatcher::commands();
        assert_eq!(
            dispatcher.match_event_with_guard(&KeyEvent::character("x"), Modifiers::ALT, false),
            Some(Command::CloseNotification)
        );
        assert_eq!(
            dispatcher.match_event_with_guard(
                &KeyEvent::character("X"),
                Modifiers::ALT | Modifiers::SHIFT,
                false
            ),
            Some(Command::CloseAllNotifications)
        );
        // Alt+C belongs to Dialog Cancel and Recent Clear, never CloseNotification.
        assert_eq!(
            dispatcher.match_event_with_guard(&KeyEvent::character("c"), Modifiers::ALT, false),
            None
        );
        assert_eq!(
            dispatcher.match_event_with_guard(
                &KeyEvent::character("C"),
                Modifiers::ALT | Modifiers::SHIFT,
                false
            ),
            None
        );
    }

    #[test]
    fn dispatcher_guards_unmodified_and_shift_bindings_when_text_entry_is_focused() {
        let dispatcher = ActionDispatcher::commands().with_binding(
            Shortcut::parse("Shift+A").expect("Shift+A"),
            Command::Cancel,
        );

        let esc = KeyEvent::named(NamedKey::Escape, PhysicalKey::Unidentified(0));
        assert_eq!(
            dispatcher.match_event_with_guard(&esc, Modifiers::NONE, false),
            Some(Command::Cancel)
        );
        assert_eq!(
            dispatcher.match_event_with_guard(&esc, Modifiers::NONE, true),
            None
        );

        let shift_a = KeyEvent::character("A");
        assert_eq!(
            dispatcher.match_event_with_guard(&shift_a, Modifiers::SHIFT, true),
            None
        );

        // Alt/Ctrl/Meta shortcuts continue firing even inside text inputs.
        let alt_n = KeyEvent::character("n");
        assert_eq!(
            dispatcher.match_event_with_guard(&alt_n, Modifiers::ALT, true),
            Some(Command::FocusNotifications)
        );
    }
}
