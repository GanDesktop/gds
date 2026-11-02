//! Decorative disclosure artwork; the host owns semantics and content lifetime.

use std::time::Duration;

use gds_uim::el;
use gpui_kit::{
    App, ElementId, IntoElement, Pixels, RenderOnce, Transformation, Window,
    assets::IconName,
    base::motion::{self, Transition},
    component::Icon,
    div, percentage,
    prelude::*,
    px,
};

const DURATION: Duration = Duration::from_millis(140);

/// A fixed-size chevron, rotating toward the host's actual expanded state.
/// Put it inside the existing labelled disclosure control, not in another hit target.
#[derive(IntoElement)]
pub struct DisclosureIcon {
    id: ElementId,
    expanded: bool,
    size: Pixels,
}

impl Default for DisclosureIcon {
    fn default() -> Self {
        Self {
            id: "disclosure".into(),
            expanded: false,
            size: px(crate::appearance::tokens::SIDEBAR_ICON_SIZE),
        }
    }
}

impl DisclosureIcon {
    /// Supply a stable identity for the section, not its position in a filtered list.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    fn progress(&self, window: &mut Window, cx: &mut App) -> f32 {
        motion::transition(
            self.id.clone(),
            if self.expanded { 1.0 } else { 0.0 },
            Transition::new(DURATION),
            window,
            cx,
        )
    }
}

impl RenderOnce for DisclosureIcon {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let progress = self.progress(window, cx);
        el! {
            .div { size: $(self.size); flex_shrink_0; } [
                (IconName::ChevronRight).Icon::new {
                    size: $(self.size);
                    transform: $(Transformation::rotate(percentage(progress * 0.25)));
                },
            ]
        }
    }
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
