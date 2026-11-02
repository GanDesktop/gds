use super::ACTION_GAP;
use crate::ui::container::container_traits;
use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

#[derive(IntoElement)]
pub struct DialogFooter {
    base: Div,
}

impl Default for DialogFooter {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_wrap; items_center; justify_end; gap: ACTION_GAP.px; } },
        }
    }
}
container_traits!(DialogFooter);
impl RenderOnce for DialogFooter {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
