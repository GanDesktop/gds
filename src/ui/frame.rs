//! Shared column geometry for the toolbar, content and footer.

use super::*;
use crate::app::AppExt as _;
use crate::appearance::tokens::{ICON_BUTTON_SIZE, SIDEBAR_WIDTH};
use gds_uim::el;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{prelude::*, *};

const MENU_PADDING: f32 = crate::appearance::tokens::CONTENT_PADDING;
const STATUS_PADDING: f32 = 18.;
const ITEM_GAP: f32 = 8.;
const DIVIDER_WIDTH: f32 = 1.;

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
            .div { absolute; left: $(px(boundary - DIVIDER_WIDTH)); top_0; bottom_0; w: DIVIDER_WIDTH.px; bg: $(cx.theme().border) }
        })
    }
}
