use gds_uim::el;
use gpui_kit::{prelude::*, *};

use super::super::container::container_traits;

/// The flexible page body; children own scrolling and selection surfaces.
#[derive(IntoElement)]
pub struct PageContent {
    base: Div,
}

impl Default for PageContent {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; flex_1; min_h_0; min_w_0; overflow_hidden; } },
        }
    }
}

container_traits!(PageContent);

impl RenderOnce for PageContent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
