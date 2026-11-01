use super::*;
use crate::appearance::tokens::SMALL;
use gds_uim::el;
use gpui_kit::component::ActiveTheme as _;

const HINT_GAP: f32 = 16.;

#[derive(Clone, IntoElement)]
pub struct ShortcutHint {
    label: SharedString,
    badge: Option<ShortcutBadge>,
}

impl ShortcutHint {
    pub fn label(&self) -> &str {
        self.label.as_ref()
    }
    pub fn plain(label: impl Into<SharedString>) -> Self {
        Self::command(label.into(), None)
    }
    pub fn new(label: impl Into<SharedString>, shortcut: impl Into<Shortcut>) -> Self {
        Self::shortcut(label.into(), shortcut.into())
    }

    pub fn binding(label: impl Into<SharedString>, binding: KeyBinding) -> Self {
        Self {
            label: label.into(),
            badge: Some(ShortcutBadge::binding(binding)),
        }
    }

    /// Mouse gestures have no keyboard action binding, but share the same keycaps.
    pub fn gesture(
        label: impl Into<SharedString>,
        keys: impl IntoIterator<Item = SharedString>,
    ) -> Self {
        Self {
            label: label.into(),
            badge: Some(ShortcutBadge::gesture(keys)),
        }
    }

    pub(in crate::ui) fn command(label: SharedString, command: Option<Shortcut>) -> Self {
        Self {
            label,
            badge: command.map(ShortcutBadge::bound),
        }
    }

    pub(in crate::ui) fn shortcut(label: SharedString, shortcut: Shortcut) -> Self {
        Self {
            label,
            badge: Some(ShortcutBadge::bound(shortcut)),
        }
    }

    pub fn builder(self) -> impl Fn(&mut Window, &mut App) -> AnyView {
        move |window, cx| {
            let hint = self.clone();
            crate::ui::tooltip::element(
                "command-tooltip",
                hint.label.clone(),
                move |_, _| hint.clone().into_any_element(),
                window,
                cx,
            )
        }
    }
}

impl RenderOnce for ShortcutHint {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let badge = self.badge.filter(|badge| !badge.labels(window).is_empty());
        el! {
            .div {
                flex; items_center; min_w_0; gap: HINT_GAP.px;
                text_size: SMALL; text_color: $(cx.theme().popover_foreground);
                line_height: $(relative(1.));
            } [
                .div { min_w_0; flex_1 } [$(self.label)],
                ..$(badge.map(|badge| el! { .div { flex_shrink_0 } [badge] })),
            ]
        }
    }
}

/// A tooltip holding a control's short label and the key that runs it.
///
/// The badge reads the highest-precedence binding like every other presentation
/// of a command, so a control whose `Command` it names cannot show another key
/// than the one a press runs.
pub fn command_tooltip(
    label: impl Into<SharedString>,
    command: impl Into<Shortcut>,
) -> impl Fn(&mut Window, &mut App) -> AnyView {
    ShortcutHint::command(label.into(), Some(command.into())).builder()
}

/// A tooltip holding a control's short label and an already-resolved binding.
///
/// It serves controls whose key is not one of the workspace [`Command`]s, such
/// as the picker's own decisions, while keeping the same presentation as every
/// other keyed control.
pub fn shortcut_tooltip(
    label: impl Into<SharedString>,
    shortcut: Shortcut,
) -> impl Fn(&mut Window, &mut App) -> AnyView {
    ShortcutHint::shortcut(label.into(), shortcut).builder()
}
