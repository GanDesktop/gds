use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

use super::super::container::container_traits;
use super::{ACTION_GAP, HEADER_PADDING_X};

/// An optional notice between the heading and content.
#[derive(IntoElement)]
pub struct PageNotice {
    base: Div,
}

impl Default for PageNotice {
    fn default() -> Self {
        Self {
            base: el! { .div { flex_shrink_0; min_w_0; px: HEADER_PADDING_X.px; pb: ACTION_GAP.px; } },
        }
    }
}

container_traits!(PageNotice);

impl RenderOnce for PageNotice {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
