//! Picker keyboard command definitions (`Back`, `Forward`, `Up`, `Refresh`, `EditLocation`, `Filter`, `Hidden`).

use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey};

use crate::ui::shortcut::Shortcut;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PickerCommand {
    Back,
    Forward,
    Up,
    Refresh,
    EditLocation,
    Filter,
    Hidden,
}

impl PickerCommand {
    /// Fluent localization message ID describing this picker command.
    #[must_use]
    pub const fn label_id(self) -> &'static str {
        match self {
            Self::Back => "picker-back",
            Self::Forward => "picker-forward",
            Self::Up => "picker-parent",
            Self::Refresh => "picker-refresh",
            Self::EditLocation => "picker-location",
            Self::Filter => "picker-filter",
            Self::Hidden => "picker-hidden",
        }
    }

    /// Default [`Shortcut`] bound to this picker command.
    #[must_use]
    pub fn default_stroke(self) -> Shortcut {
        match self {
            Self::Back => Shortcut::new(Key::Named(NamedKey::ArrowLeft), Modifiers::ALT),
            Self::Forward => Shortcut::new(Key::Named(NamedKey::ArrowRight), Modifiers::ALT),
            Self::Up => Shortcut::new(Key::Named(NamedKey::ArrowUp), Modifiers::ALT),
            Self::Refresh => Shortcut::new(Key::character("r"), Modifiers::PRIMARY),
            Self::EditLocation => Shortcut::new(Key::character("l"), Modifiers::PRIMARY),
            Self::Filter => Shortcut::new(Key::character("f"), Modifiers::PRIMARY),
            Self::Hidden => Shortcut::new(Key::character("h"), Modifiers::PRIMARY),
        }
    }

    /// Returns `true` when `event` and `modifiers` match this picker command (including `F5` for
    /// `Refresh`).
    #[must_use]
    pub fn matches_event(self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        if self.default_stroke().matches_event(event, modifiers) {
            return true;
        }
        if self == Self::Refresh
            && !event.repeat
            && modifiers.held().is_empty()
            && event.key == Key::Named(NamedKey::F5)
        {
            return true;
        }
        false
    }
}
