use gds_uim::el;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _};
use gpui_kit::{prelude::*, *};

use crate::ui::container::container_traits;

/// Secondary setting text; validation and persistence remain with the caller.
#[derive(IntoElement)]
pub struct SettingDescription {
    base: Div,
}

impl Default for SettingDescription {
    fn default() -> Self {
        Self {
            base: el! { .div { text_sm; } },
        }
    }
}

container_traits!(SettingDescription);

impl RenderOnce for SettingDescription {
    fn render(mut self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let style = std::mem::take(self.base.style());
        el! { $(self.base) { text_color: $(cx.theme().muted_foreground); refine_style: $&style; } }
    }
}
