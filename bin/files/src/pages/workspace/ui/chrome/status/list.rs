use gds_uim::{component, el};
use gpui_kit::{IntoElement, prelude::*, *};

const LIST_WIDTH: f32 = 320.;
const LIST_HEIGHT: f32 = 320.;

#[component]
#[derive(Default, IntoElement)]
pub(in crate::pages::workspace) struct StatusList {
    #[attr(into)]
    empty: SharedString,
    rows: Vec<AnyElement>,
}

impl ParentElement for StatusList {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.rows.extend(elements);
    }
}

impl RenderOnce for StatusList {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        el! {
            .div {
                # "status-details"; w: LIST_WIDTH.px; max_w_full; max_h: LIST_HEIGHT.px;
                overflow_y_scroll; flex; flex_col; p_1; text_sm;
            } [
                @if (self.rows.is_empty()) .div [$(self.empty)], @end,
                ..$(self.rows),
            ]
        }
    }
}
