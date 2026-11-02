use gds_uim::el;
use gpui_kit::component::StyledExt as _;
use gpui_kit::{IntoElement, prelude::*, *};

use super::super::super::container::container_traits;
use super::super::TITLE_SIZE;

/// Page-title typography following the configured application text scale.
#[derive(IntoElement)]
pub struct PageTitle {
    base: Div,
}

impl Default for PageTitle {
    fn default() -> Self {
        Self {
            base: el! { .div { flex_1; min_w_0; truncate; font_weight: FontWeight::SEMIBOLD; } },
        }
    }
}

container_traits!(PageTitle);

impl RenderOnce for PageTitle {
    fn render(mut self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let style = std::mem::take(self.base.style());
        el! { $(self.base) { text_size: $(crate::appearance::typography::scaled(TITLE_SIZE, cx)); refine_style: $&style; } }
    }
}
