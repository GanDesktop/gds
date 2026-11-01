use super::{ITEM_GAP, SIDEBAR_ITEM_HEIGHT};
use crate::ui::container::container_traits;
use gds_uim::el;
use gpui_kit::{prelude::*, *};
#[derive(IntoElement)]
pub struct NavItemHeader {
    base: Div,
}

impl Default for NavItemHeader {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; w_full; min_w_0; flex_shrink_0; items_center; gap: ITEM_GAP.px; h: SIDEBAR_ITEM_HEIGHT.px; px: ITEM_GAP.px; } },
        }
    }
}
container_traits!(NavItemHeader);
impl RenderOnce for NavItemHeader {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
