use crate::ui::container::container_traits;
use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

#[derive(IntoElement)]
pub struct NavItemLabel {
    base: Div,
}

impl Default for NavItemLabel {
    fn default() -> Self {
        Self {
            base: el! { .div { min_w_0; text_ellipsis; } },
        }
    }
}
container_traits!(NavItemLabel);
impl RenderOnce for NavItemLabel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
