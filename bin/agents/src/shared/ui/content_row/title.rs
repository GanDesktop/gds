use gds_uim::el;
use gpui_kit::{prelude::*, *};

use super::row_traits;

/// The row's primary text, inheriting its content typography.
#[derive(IntoElement)]
pub(crate) struct ContentRowTitle {
    base: Div,
}

impl Default for ContentRowTitle {
    fn default() -> Self {
        Self {
            base: el! { .div { min_w_0; truncate; } },
        }
    }
}

row_traits!(ContentRowTitle);

impl RenderOnce for ContentRowTitle {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
