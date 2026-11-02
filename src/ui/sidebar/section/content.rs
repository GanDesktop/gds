use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

use super::super::super::container::container_traits;

/// A column for arbitrary section children. Scrolling is opt-in and requires an ID.
#[derive(IntoElement)]
pub struct SideBarSectionContent {
    base: Div,
}

impl Default for SideBarSectionContent {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; w_full; min_w_0; } },
        }
    }
}

container_traits!(SideBarSectionContent);

impl RenderOnce for SideBarSectionContent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
