//! Shared column geometry for the toolbar, content and footer.

use super::*;
use crate::appearance::theme;
use crate::appearance::tokens::{ICON_BUTTON_SIZE, SIDEBAR_WIDTH};
use gds_uim::el;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{prelude::*, *};

const MENU_PADDING: f32 = crate::appearance::tokens::CONTENT_PADDING;
const STATUS_PADDING: f32 = 18.;
const ITEM_GAP: f32 = 8.;
const DIVIDER_WIDTH: f32 = 1.;
const MIN_DRAG_WIDTH: f32 = 8.;

mod drag;
mod footer;
mod toolbar;
mod workspace;

pub use drag::WindowDragArea;

/// One shared column width governs toolbar, workspace and footer geometry.
#[gds_uim::component]
pub struct Frame {
    #[attr(into, option)]
    sidebar_width: Option<f32>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            sidebar_width: Some(SIDEBAR_WIDTH),
        }
    }
}

impl Frame {
    /// A full-width page whose toolbar has no sidebar column or divider.
    pub fn without_sidebar() -> Self {
        Self {
            sidebar_width: None,
        }
    }

    fn vertical_divider(&self, cx: &App) -> Option<Div> {
        self.sidebar_width.map(|boundary| el! {
            .div { absolute; left: boundary.px; top_0; bottom_0; w: DIVIDER_WIDTH.px; bg: $(cx.theme().border) }
        })
    }

    /// The toolbar's empty stretch after its leading control, when the column
    /// is wide enough to leave one.
    ///
    /// The leading control is an [`ICON_BUTTON_SIZE`] control in both of the
    /// toolbar's forms (the menu and the secondary page's way back), so the
    /// space beside it is the column width minus the control and its padding.
    fn window_drag_area(&self) -> Option<WindowDragArea> {
        let free = self.sidebar_width? - 2. * MENU_PADDING - ICON_BUTTON_SIZE;
        (free >= MIN_DRAG_WIDTH).then_some(WindowDragArea)
    }
}
