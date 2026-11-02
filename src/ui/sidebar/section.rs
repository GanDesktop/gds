mod content;
mod header;
mod title;

use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

use super::super::container::container_traits;
pub use content::SideBarSectionContent;
pub use header::SideBarSectionHeader;
pub use title::SideBarSectionTitle;

/// A content group, usually composed from a section header and section content.
#[derive(IntoElement)]
pub struct SideBarSection {
    base: Div,
}

impl Default for SideBarSection {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; flex_shrink_0; w_full; min_w_0; } },
        }
    }
}

container_traits!(SideBarSection);

impl RenderOnce for SideBarSection {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
