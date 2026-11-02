mod actions;
mod title;

pub use actions::PageActions;
use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};
pub use title::PageTitle;

use super::super::container::container_traits;
use super::{ACTION_GAP, HEADER_PADDING_X, HEADER_PADDING_Y};

/// The page heading row, holding a title and optional actions.
#[derive(IntoElement)]
pub struct PageHeader {
    base: Div,
}

impl Default for PageHeader {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_shrink_0; items_center; gap: ACTION_GAP.px; px: HEADER_PADDING_X.px; py: HEADER_PADDING_Y.px; } },
        }
    }
}

container_traits!(PageHeader);

impl RenderOnce for PageHeader {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
