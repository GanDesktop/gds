use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

use super::row_traits;

/// The flexible content column.
#[derive(IntoElement)]
pub(crate) struct ContentRowContent {
    base: Div,
}

impl Default for ContentRowContent {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; flex_1; min_w_0; } },
        }
    }
}

row_traits!(ContentRowContent);

impl RenderOnce for ContentRowContent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
