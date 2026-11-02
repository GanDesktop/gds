use super::*;
use crate::appearance::tokens::{SIDEBAR_PADDING, TOOLBAR_HEIGHT};
use gds_uim::el;

impl Frame {
    /// The window's top row: the menu column, the page controls, and the
    /// column divider the workspace and footer draw at the same coordinate.
    ///
    /// Empty space across the entire row moves the window; controls claim
    /// their mouse presses before the drag background receives them.
    pub fn toolbar(&self, menu: AnyElement, controls: AnyElement, cx: &App) -> Div {
        self.toolbar_with_window_controls(menu, controls, window::WindowControls::default(), cx)
    }

    /// Applications supply lifecycle callbacks; Frame owns the placement so a
    /// full-width title or navigation row cannot displace the window controls.
    pub fn toolbar_with_window_controls(
        &self,
        menu: AnyElement,
        controls: AnyElement,
        window_controls: window::WindowControls,
        cx: &App,
    ) -> Div {
        let width = self
            .sidebar_width
            .unwrap_or(2. * MENU_PADDING + ICON_BUTTON_SIZE);
        let leading_background = if self.sidebar_width.is_some() {
            theme::sidebar_background(cx)
        } else {
            theme::content_background(cx)
        };
        let divider = self.vertical_divider(cx);
        el! {
            .div { relative; flex; flex_shrink_0; w_full; h: TOOLBAR_HEIGHT.px } [
                .div { absolute; inset_0 } [WindowDragArea],
                .div {
                    w: width.px; h_full; flex_shrink_0; flex; items_center;
                    px: MENU_PADDING.px; bg: leading_background;
                    @if (width <= crate::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH) justify_center; @end;
                } [
                    menu,
                ],
                .div {
                    flex_1; min_w_0; h_full; flex; items_center;
                    px: SIDEBAR_PADDING.px; gap_1; bg: $(theme::content_background(cx));
                } [
                    .div { flex_1; min_w_0; h_full; flex; items_center; overflow_hidden } [controls],
                    window_controls,
                ],
                ..divider,
            ]
        }
    }
}
