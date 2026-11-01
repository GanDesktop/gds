use super::DIALOG_CONTEXT;
use crate::ui::Shortcut;
use gpui_kit::component::dialog::{Cancel, Confirm};
use gpui_kit::*;

/// The two decisions every dialog offers, each with one shared binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogCommand {
    Confirm,
    Cancel,
}

impl DialogCommand {
    pub const ALL: [Self; 2] = [Self::Confirm, Self::Cancel];

    pub fn label_id(self) -> &'static str {
        match self {
            Self::Confirm => "dialog-confirm",
            Self::Cancel => "dialog-cancel",
        }
    }

    pub fn default_stroke(self) -> &'static str {
        match self {
            Self::Confirm => "alt-a",
            Self::Cancel => "alt-c",
        }
    }

    /// The existing `gpui-base` actions, so the keyboard path and the click
    /// path reach the same dialog callbacks.
    pub fn action(self) -> Box<dyn Action> {
        match self {
            Self::Confirm => Box::new(Confirm { secondary: false }),
            Self::Cancel => Box::new(Cancel),
        }
    }

    pub fn context(self) -> Option<&'static str> {
        Some(DIALOG_CONTEXT)
    }

    pub fn shortcut(self) -> Shortcut {
        Shortcut::new(self.action(), self.context())
    }

    pub(super) fn button_id(self) -> &'static str {
        match self {
            Self::Confirm => "dialog-confirm",
            Self::Cancel => "dialog-cancel",
        }
    }

    /// Distinct from [`Self::button_id`], whose element state is the button's
    /// own focus handle.
    pub(super) fn anchor_key(self) -> &'static str {
        match self {
            Self::Confirm => "dialog-confirm-anchor",
            Self::Cancel => "dialog-cancel-anchor",
        }
    }

    /// Distinct from [`Self::anchor_key`], whose element state is the focus
    /// handle; one element path cannot hold both states.
    pub(super) fn tooltip_id(self) -> &'static str {
        match self {
            Self::Confirm => "dialog-confirm-hint",
            Self::Cancel => "dialog-cancel-hint",
        }
    }

    pub(super) fn binding(self, cx: &App) -> KeyBinding {
        let context = KeyBindingContextPredicate::parse(DIALOG_CONTEXT)
            .expect("bundled action contexts are valid")
            .into();
        KeyBinding::load(
            self.default_stroke(),
            self.action(),
            Some(context),
            false,
            None,
            cx,
        )
        .expect("bundled dialog shortcuts are valid")
    }
}
