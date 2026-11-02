use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

use super::super::CONTROL_WIDTH;
use crate::ui::container::container_traits;

/// The shared control column, keeping controls aligned between rows.
#[derive(IntoElement)]
pub struct SettingControl {
    base: Div,
}

impl Default for SettingControl {
    fn default() -> Self {
        Self {
            base: el! { .div { w: CONTROL_WIDTH.px; flex; flex_shrink_0; items_center; justify_end; } },
        }
    }
}

container_traits!(SettingControl);

impl RenderOnce for SettingControl {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
