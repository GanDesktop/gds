/// Outcome produced when a dialog button or shortcut completes a prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogDecision {
    Confirm,
    Cancel,
    Dismiss,
}

impl DialogDecision {
    /// Returns `true` when the user confirmed the dialog action.
    #[must_use]
    pub const fn is_confirmed(self) -> bool {
        matches!(self, Self::Confirm)
    }

    /// Returns `true` when the user cancelled or dismissed the dialog.
    #[must_use]
    pub const fn is_cancelled(self) -> bool {
        !self.is_confirmed()
    }
}
