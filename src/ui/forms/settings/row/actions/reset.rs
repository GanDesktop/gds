use gds_uim::el;
use gpui_kit::{prelude::*, *};

use super::super::super::RESET_WIDTH;
use crate::ui::container::container_traits;

/// Reserves the reset target's space even when no action is shown.
#[derive(IntoElement)]
pub struct SettingReset {
    base: Div,
}

impl Default for SettingReset {
    fn default() -> Self {
        Self {
            base: el! { .div { w: RESET_WIDTH.px; h: RESET_WIDTH.px; flex; flex_shrink_0; items_center; justify_center; } },
        }
    }
}

container_traits!(SettingReset);

impl RenderOnce for SettingReset {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
