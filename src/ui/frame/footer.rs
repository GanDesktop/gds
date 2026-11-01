use super::*;
use crate::appearance::tokens::FOOTER_HEIGHT;
use gds_uim::el;

impl Frame {
    /// The window-wide bottom bar keeps sidebar context at the far left and
    /// window-level controls at the far right, including when a dock is open.
    pub fn footer(
        &self,
        sidebar_status: AnyElement,
        status: AnyElement,
        controls: AnyElement,
        cx: &App,
    ) -> Div {
        let width = self.sidebar_width;
        let boundary = width.unwrap_or(0.);
        let divider = self.vertical_divider(cx);
        el! {
            .div { relative; flex; w_full; h: FOOTER_HEIGHT.px; flex_shrink_0 } [
                @if (let Some(width) = width)
                    .div { w: width.px; h_full; flex_shrink_0; bg: $(theme::sidebar_background(cx)) } [sidebar_status],
                @end,
                .div {
                    flex_1; min_w_0; h_full; flex; items_center; gap: ITEM_GAP.px;
                    bg: $(theme::content_background(cx)); px: STATUS_PADDING.px;
                } [
                    .div { flex_1; min_w_0; flex; items_center; gap: ITEM_GAP.px } [status],
                    controls,
                ],
                ..divider,
                .div { absolute; left: boundary.px; right_0; top_0; h: DIVIDER_WIDTH.px; bg: $(cx.theme().border) },
            ]
        }
    }
}
