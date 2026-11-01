use crate::ui::container::container_traits;
use gds_uim::el;
use gpui_kit::{prelude::*, *};

#[derive(IntoElement)]
pub struct NavItemDetails {
    base: Div,
}

impl Default for NavItemDetails {
    fn default() -> Self {
        Self {
            base: el! { .div { w_full; min_w_0; } },
        }
    }
}
container_traits!(NavItemDetails);
impl RenderOnce for NavItemDetails {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
