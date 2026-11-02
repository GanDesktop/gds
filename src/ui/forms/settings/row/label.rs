use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

use crate::ui::container::container_traits;

/// The flexible label column, optionally containing a description.
#[derive(IntoElement)]
pub struct SettingLabel {
    base: Div,
}

impl Default for SettingLabel {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; flex_1; min_w_0; text_sm; } },
        }
    }
}

container_traits!(SettingLabel);

impl RenderOnce for SettingLabel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
