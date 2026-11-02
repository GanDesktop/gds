use super::keycap::Keycap;
use super::*;
use gds_uim::el;
use gpui_kit::IntoElement;

#[derive(Clone, IntoElement)]
pub struct ShortcutBadge {
    source: BadgeSource,
}

/// Where a badge's keys come from: a control's own shortcut, resolved in its
/// declared scope, or a binding already resolved for the current context.
#[derive(Clone)]
enum BadgeSource {
    Shortcut(Shortcut),
    Binding(KeyBinding),
    Gesture(Vec<SharedString>),
}

impl ShortcutBadge {
    pub(super) fn gesture(keys: impl IntoIterator<Item = SharedString>) -> Self {
        Self {
            source: BadgeSource::Gesture(keys.into_iter().collect()),
        }
    }
    pub fn new(command: impl Into<Shortcut>) -> Self {
        Self::bound(command.into())
    }

    /// A badge for any action, resolved through its own key context.
    pub fn bound(shortcut: Shortcut) -> Self {
        Self {
            source: BadgeSource::Shortcut(shortcut),
        }
    }

    /// A badge for a binding that is already known to apply right now.
    pub fn binding(binding: KeyBinding) -> Self {
        Self {
            source: BadgeSource::Binding(binding),
        }
    }

    pub(super) fn labels(&self, window: &Window) -> Vec<SharedString> {
        let binding = match &self.source {
            BadgeSource::Shortcut(shortcut) => shortcut.binding(window),
            BadgeSource::Binding(binding) => Some(binding.clone()),
            BadgeSource::Gesture(keys) => return keys.clone(),
        };
        key_labels(binding.as_ref())
    }

    /// The whole combination as one compact label, such as `R` or `Shift+Tab`,
    /// with the modifier the user is already holding left out because the press
    /// itself shows it.
    ///
    /// It is spelled from [`key_labels`], the same names the keycaps use, so the
    /// two presentations cannot drift apart.
    pub(crate) fn path_label(bindings: &[KeyBinding], held: HeldModifier) -> Option<SharedString> {
        let labels = bindings
            .iter()
            .flat_map(|binding| key_labels_omitting(Some(binding), Some(held)))
            .collect::<Vec<_>>();
        if labels.is_empty() {
            return None;
        }
        let parts = labels.iter().map(AsRef::<str>::as_ref).collect::<Vec<_>>();
        Some(parts.join("+").into())
    }
}

impl RenderOnce for ShortcutBadge {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let keys = self.labels(window).into_iter().map(Keycap::new);
        el! { .div { flex; items_center; gap: KEY_GAP.px } [..keys] }
    }
}
