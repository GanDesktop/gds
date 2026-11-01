//! Shared dialog decisions preserve the modal's own callbacks and key bindings.

mod anchor;
mod command;
mod decision;
mod footer;

pub use command::DialogCommand;
pub use decision::DialogDecision;
pub use footer::DialogFooter;

use gds_uim::component;
use gpui_kit::assets::IconName;
use gpui_kit::component::{StyledExt as _, button::ButtonVariant};
use gpui_kit::{prelude::*, *};

pub const DIALOG_CONTEXT: &str = "Dialog";
const ACTION_GAP: f32 = 8.;

/// Bind the dialog decisions once at startup.
pub fn register(cx: &mut App) {
    let bindings = DialogCommand::ALL
        .into_iter()
        .map(|command| command.binding(cx))
        .collect::<Vec<_>>();
    cx.bind_keys(bindings);
}

/// The standard confirm/cancel composition; labels and appearance are optional overrides.
#[component]
#[derive(IntoElement)]
pub struct DialogActions {
    #[attr(into, option)]
    primary_label: Option<SharedString>,
    #[attr]
    primary_icon: IconName,
    #[attr(into, option)]
    cancel_label: Option<SharedString>,
    danger: bool,
    style: StyleRefinement,
}

impl Default for DialogActions {
    fn default() -> Self {
        Self {
            primary_label: None,
            primary_icon: IconName::Check,
            cancel_label: None,
            danger: false,
            style: StyleRefinement::default(),
        }
    }
}

impl DialogActions {
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

impl Styled for DialogActions {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for DialogActions {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut primary = DialogDecision::new(DialogCommand::Confirm)
            .icon(self.primary_icon)
            .variant(if self.danger {
                ButtonVariant::Danger
            } else {
                ButtonVariant::Primary
            });
        if let Some(label) = self.primary_label {
            primary = primary.label(label);
        }
        let mut cancel = DialogDecision::new(DialogCommand::Cancel);
        if let Some(label) = self.cancel_label {
            cancel = cancel.label(label);
        }
        DialogFooter::default()
            .refine_style(&self.style)
            .child(cancel)
            .child(primary)
    }
}

#[cfg(test)]
mod tests;
