//! Composable sidebar anatomy. Applications own state and the content of each part.

mod content;
mod footer;
mod header;
mod presentation;
mod section;

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

use gds_uim::{component, el};
use gpui_kit::component::StyledExt;
use gpui_kit::{IntoElement, prelude::*, *};

use super::container::container_traits;
pub use content::SideBarContent;
pub use footer::SideBarFooter;
pub use header::SideBarHeader;
pub use presentation::SidebarPresentation;
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
/// Defaults to expanded. Static callers use state directly; animated hosts call
/// `sample` once and pass that presentation to all column parts, including this root.
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
    #[attr(option)]
    presentation: Option<SidebarPresentation>,
    base: Div,
}

impl Default for SideBar {
    fn default() -> Self {
        Self {
            state: SidebarState::default(),
            presentation: None,
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
    /// The presented width, or the immediate state width for static callers.
    pub fn width(&self) -> f32 {
        self.presentation
            .map_or_else(|| self.target_width(), |value| value.width())
    }

    fn target_width(&self) -> f32 {
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
