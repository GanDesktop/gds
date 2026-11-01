use super::ContentPage;
use crate::ui::container::container_traits;
use gds_uim::el;
use gpui_kit::{prelude::*, *};
#[derive(IntoElement)]
pub struct PageTableArea {
    base: Div,
}

impl Default for PageTableArea {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; flex_1; min_h_0; min_w_0; overflow_hidden; px: ContentPage::TABLE_INSET.px; } },
        }
    }
}
container_traits!(PageTableArea);
impl RenderOnce for PageTableArea {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
