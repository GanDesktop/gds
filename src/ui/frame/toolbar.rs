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
        let theme = cx.gds_ro().appearance().theme();
        let leading_background = if self.sidebar_width.is_some() {
            theme.sidebar_background()
        } else {
            theme.content_background()
        };
        let divider = self.vertical_divider(cx);
        let collapsed_width = crate::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH;
        let progress =
            ((width - collapsed_width) / (SIDEBAR_WIDTH - collapsed_width)).clamp(0.0, 1.0);
        let collapsed_inset = (collapsed_width - ICON_BUTTON_SIZE) / 2.0;
        let menu_inset = if self.sidebar_width.is_some() {
            collapsed_inset + (MENU_PADDING - collapsed_inset) * progress
        } else {
            MENU_PADDING
        };
        el! {
            .div { relative; flex; flex_shrink_0; w_full; h: TOOLBAR_HEIGHT.px } [
                .div { absolute; inset_0 } [WindowDragArea],
                .div {
                    # "frame-toolbar-sidebar"; test_support;
                    w: width.px; h_full; flex_shrink_0; flex; items_center;
                    pl: menu_inset.px; pr: MENU_PADDING.px; bg: leading_background;
                } [
                    menu,
                ],
                .div {
                    flex_1; min_w_0; h_full; flex; items_center;
                    px: SIDEBAR_PADDING.px; gap_1; bg: $(theme.content_background());
                } [
                    .div { flex_1; min_w_0; h_full; flex; items_center; overflow_hidden } [controls],
                    window_controls,
                ],
                ..divider,
            ]
        }
    }
}
