use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

use super::super::super::container::container_traits;
use super::super::ACTION_GAP;

/// Actions that keep their own commands, tooltips and shortcuts.
#[derive(IntoElement)]
pub struct PageActions {
    base: Div,
}

impl Default for PageActions {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; items_center; flex_shrink_0; gap: ACTION_GAP.px; } },
        }
    }
}

container_traits!(PageActions);

impl RenderOnce for PageActions {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
