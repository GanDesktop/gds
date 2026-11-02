//! Composable content-page anatomy for directories and collection tables.

mod content;
mod header;
mod notice;
pub mod stack;
mod table;

use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

use super::container::container_traits;
pub use content::PageContent;
pub use header::{PageActions, PageHeader, PageTitle};
pub use notice::PageNotice;
pub use table::PageTableArea;

const HEADER_PADDING_X: f32 = 24.;
const HEADER_PADDING_Y: f32 = 16.;
const TITLE_SIZE: f32 = 22.;
const ACTION_GAP: f32 = 8.;

/// A page column composed from headers, notices and flexible content.
#[derive(IntoElement)]
pub struct ContentPage {
    base: Div,
}

impl Default for ContentPage {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; flex_1; size_full; min_h_0; min_w_0; } },
        }
    }
}

container_traits!(ContentPage);

impl ContentPage {
    pub const TABLE_INSET: f32 = 18.;
}

impl RenderOnce for ContentPage {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
