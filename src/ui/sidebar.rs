//! Composable sidebar anatomy. Applications own state and the content of each part.

mod content;
mod footer;
mod header;
mod section;

use gds_uim::{component, el};
use gpui_kit::component::StyledExt;
use gpui_kit::{prelude::*, *};

use super::container::container_traits;
pub use content::SideBarContent;
pub use footer::SideBarFooter;
pub use header::SideBarHeader;
pub use section::{
    SideBarSection, SideBarSectionContent, SideBarSectionHeader, SideBarSectionTitle,
};

/// Controlled by the application so layout, shortcuts and persistence agree.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SidebarState {
    #[default]
    Expanded,
    Collapsed,
}

/// Root column containing headers, content, footers or other application elements.
///
/// Defaults to expanded. State selects the column width; callers compose the
/// contents for that state.
/// Use `Frame::default().sidebar_width(sidebar.width())` to share the column boundary.
#[component]
#[derive(IntoElement)]
pub struct SideBar {
    #[attr]
    state: SidebarState,
    #[attr(into)]
    expanded_width: f32,
    #[attr(into)]
    collapsed_width: f32,
    base: Div,
}

impl Default for SideBar {
    fn default() -> Self {
        Self {
            state: SidebarState::default(),
            expanded_width: crate::appearance::tokens::SIDEBAR_WIDTH,
            collapsed_width: crate::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH,
            base: el! { .div {
                flex; flex_col; flex_shrink_0;
                h_full; min_w_0; min_h_0; overflow_hidden;
            } },
        }
    }
}

impl SideBar {
    /// The width for the current state, including custom width settings.
    pub fn width(&self) -> f32 {
        match self.state {
            SidebarState::Expanded => self.expanded_width,
            SidebarState::Collapsed => self.collapsed_width,
        }
    }
}

container_traits!(SideBar);

impl RenderOnce for SideBar {
    fn render(mut self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let width = self.width();
        let style = std::mem::take(self.base.style());
        el! { $(self.base) { w: width.px; refine_style: $&style; } }
    }
}
