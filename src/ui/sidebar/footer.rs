use gds_uim::el;
use gpui_kit::{prelude::*, *};

use super::super::container::container_traits;

/// A column below the scrolling content that keeps its natural height.
#[derive(IntoElement)]
pub struct SideBarFooter {
    base: Div,
}

impl Default for SideBarFooter {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; flex_shrink_0; w_full; min_w_0; } },
        }
    }
}

container_traits!(SideBarFooter);

impl RenderOnce for SideBarFooter {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
