use zgui::prelude::*;
use zgui::view::current_host;

/// Focus anchor restored when a modal dialog closes.
#[derive(Clone, Copy, Default)]
pub struct DialogAnchor {
    pub trigger: NodeRef,
}

impl DialogAnchor {
    /// Creates an anchor targeting `trigger`.
    #[must_use]
    pub fn new(trigger: NodeRef) -> Self {
        Self { trigger }
    }

    /// Captures the currently focused element in the active window, if any.
    #[must_use]
    pub fn capture() -> Option<zgui::view::NodeId> {
        current_host().and_then(|host| host.focused().get_untracked())
    }

    /// Returns focus to `self.trigger` if bound in the window.
    pub fn restore(self) {
        self.trigger.focus();
    }
}
