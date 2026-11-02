use super::{DialogCommand, anchor::DispatchAnchor};
use crate::ui::button::Button;
use crate::{
    appearance::tokens::TOOLTIP_DELAY,
    ui::{ShortcutHint as Hint, keytips},
};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon,
    button::{ButtonVariant, ButtonVariants},
};
use gpui_kit::{IntoElement, prelude::*, *};

/// One dialog decision as rendered in a footer.
#[gds_uim::component]
#[derive(IntoElement)]
pub struct DialogDecision {
    command: DialogCommand,
    #[attr(into, option)]
    label: Option<SharedString>,
    #[attr(option)]
    icon: Option<IconName>,
    #[attr]
    variant: ButtonVariant,
}

impl DialogDecision {
    pub fn new(command: DialogCommand) -> Self {
        Self {
            command,
            label: None,
            icon: None,
            variant: if command == DialogCommand::Confirm {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Default
            },
        }
    }
}
impl RenderOnce for DialogDecision {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let anchor = DispatchAnchor::new(self.command.anchor_key(), window, cx);
        let action = self.command.action();
        let label = self
            .label
            .unwrap_or_else(|| crate::i18n::text(self.command.label_id(), cx));
        let icon = self
            .icon
            .unwrap_or(if self.command == DialogCommand::Confirm {
                IconName::Check
            } else {
                IconName::X
            });
        // Only the open dialog's own decisions carry a binding, so the label
        // layer shows Alt+A / Alt+C here and no workspace key behind the modal.
        // The slot sits after the label inside the button, so a hint never
        // covers the button's text or the count in a destructive confirmation,
        // and its stable width keeps the footer from reflowing.
        let hint = keytips::suffix_anchor(
            window,
            cx,
            self.command.button_id().into(),
            self.command.shortcut(),
        );
        let button = Button::new(self.command.button_id())
            .with_variant(self.variant)
            .icon(Icon::new(icon))
            .label(label.clone())
            .child(anchor.element())
            .child(hint)
            .on_click(move |_, window, cx| anchor.dispatch(action.as_ref(), window, cx));
        let tooltip = Hint::shortcut(label, self.command.shortcut()).builder();
        div()
            .id(self.command.tooltip_id())
            .flex_shrink_0()
            .tooltip_show_delay(TOOLTIP_DELAY)
            .tooltip(tooltip)
            .child(button)
    }
}
