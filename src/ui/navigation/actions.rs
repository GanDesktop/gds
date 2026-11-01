use crate::appearance::tokens::{ICON_BUTTON_SIZE, SIDEBAR_ITEM_HEIGHT};
use crate::ui::container::container_traits;
use gds_uim::el;
use gpui_kit::{prelude::*, *};
#[derive(IntoElement)]
pub struct NavItemActions {
    base: Div,
}

impl Default for NavItemActions {
    fn default() -> Self {
        Self {
            base: el! { .div { absolute; right_0; top: $px((SIDEBAR_ITEM_HEIGHT - ICON_BUTTON_SIZE) / 2.); flex; } },
        }
    }
}
container_traits!(NavItemActions);
impl RenderOnce for NavItemActions {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
