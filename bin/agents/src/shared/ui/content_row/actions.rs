use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

use super::row_traits;

/// Independent controls that keep their natural width.
#[derive(IntoElement)]
pub(crate) struct ContentRowActions {
    base: Div,
}

impl Default for ContentRowActions {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; items_center; flex_shrink_0; gap_1; } },
        }
    }
}

row_traits!(ContentRowActions);

impl RenderOnce for ContentRowActions {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
