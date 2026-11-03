use super::{DialogCommand, anchor::DispatchAnchor};
use crate::ui::button::Button;
use crate::{appearance::tokens::TOOLTIP_DELAY, ui::ShortcutHint as Hint};
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
        let button = Button::new(self.command.button_id())
            .with_variant(self.variant)
            .icon(Icon::new(icon))
            .label(label.clone())
            .child(anchor.element())
            .shortcut(self.command.shortcut())
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
