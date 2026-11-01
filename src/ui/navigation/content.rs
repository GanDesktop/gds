use crate::ui::container::container_traits;
use gds_uim::el;
use gpui_kit::{prelude::*, *};

#[derive(IntoElement)]
pub struct NavItemContent {
    base: Div,
}

impl Default for NavItemContent {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; w_full; min_w_0; } },
        }
    }
}
container_traits!(NavItemContent);
impl RenderOnce for NavItemContent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
