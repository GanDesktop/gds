use gds_uim::el;
use gpui_kit::{prelude::*, *};

use super::row_traits;

/// Optional leading icons or other compact content.
#[derive(IntoElement)]
pub(crate) struct ContentRowLeading {
    base: Div,
}

impl Default for ContentRowLeading {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_shrink_0; items_center; } },
        }
    }
}

row_traits!(ContentRowLeading);

impl RenderOnce for ContentRowLeading {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
