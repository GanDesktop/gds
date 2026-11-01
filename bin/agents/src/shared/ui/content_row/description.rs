use gds_uim::el;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _};
use gpui_kit::{prelude::*, *};

use super::row_traits;

/// Secondary text using Agents' compact interface typography.
#[derive(IntoElement)]
pub(crate) struct ContentRowDescription {
    base: Div,
}

impl Default for ContentRowDescription {
    fn default() -> Self {
        Self {
            base: el! { .div { min_w_0; truncate; text_size: crate::shared::ui::SMALL; } },
        }
    }
}

row_traits!(ContentRowDescription);

impl RenderOnce for ContentRowDescription {
    fn render(mut self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let style = std::mem::take(self.base.style());
        el! { $(self.base) { text_color: $(cx.theme().muted_foreground); refine_style: $&style; } }
    }
}
