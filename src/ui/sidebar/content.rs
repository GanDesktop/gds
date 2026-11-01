use gds_uim::el;
use gpui_kit::{prelude::*, *};

use super::super::container::container_traits;

/// Fills the remaining height and scrolls vertically. Use `overflow_hidden()`
/// when its children own scrolling, such as independently scrollable sections.
/// For scrolling, provide a stable `id()` or an explicit `track_scroll()` handle.
#[derive(IntoElement)]
pub struct SideBarContent {
    base: Div,
}

impl Default for SideBarContent {
    fn default() -> Self {
        Self {
            base: el! { .div {
                flex; flex_col; flex_1; min_w_0; min_h_0;
            } },
        }
        .overflow_y_scroll()
    }
}

container_traits!(SideBarContent);

impl RenderOnce for SideBarContent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
