use gds_uim::el;
use gpui_kit::{prelude::*, *};

use super::super::container::container_traits;

/// A column above the scrolling content that keeps its natural height.
#[derive(IntoElement)]
pub struct SideBarHeader {
    base: Div,
}

impl Default for SideBarHeader {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; flex_shrink_0; w_full; min_w_0; } },
        }
    }
}

container_traits!(SideBarHeader);

impl RenderOnce for SideBarHeader {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
