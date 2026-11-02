use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

use super::super::super::container::container_traits;

/// A row for the section title, actions or a custom disclosure control.
#[derive(IntoElement)]
pub struct SideBarSectionHeader {
    base: Div,
}

impl Default for SideBarSectionHeader {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; items_center; flex_shrink_0; w_full; min_w_0; } },
        }
    }
}

container_traits!(SideBarSectionHeader);

impl RenderOnce for SideBarSectionHeader {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
