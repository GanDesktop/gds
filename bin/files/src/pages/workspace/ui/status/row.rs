use gds_uim::{component, el, m};
use gpui_kit::{
    IntoElement,
    component::{list::ListItem, tooltip::Tooltip},
    prelude::*,
    *,
};

const ROW_HEIGHT: f32 = 32.;

#[component(gpui = gpui_kit)]
#[derive(Default, IntoElement)]
pub(in crate::pages::workspace::ui) struct StatusRow {
    #[attr(into)]
    id: SharedString,
    #[attr(into)]
    title: SharedString,
    #[attr(any_element, option)]
    content: Option<AnyElement>,
    #[attr(any_element, option)]
    action: Option<AnyElement>,
}

impl RenderOnce for StatusRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let tooltip = move |window: &mut Window, cx: &mut App| {
            Tooltip::new(self.title.clone()).build(window, cx)
        };
        el! {
            $(self.id).ListItem::new {
                group: "status-row"; h: ROW_HEIGHT.px; w_full; px_2; py_0;
                text_sm; rounded_md; tooltip: tooltip;
            } [
                .div { flex; items_center; gap_2; w_full; min_w_0; } [
                    .div { flex_1; min_w_0; } [
                        @if (let Some(content) = self.content) content, @end,
                    ],
                    .div { flex_shrink_0; invisible; group_hover: "status-row", m! { visible; }; } [
                        @if (let Some(action) = self.action) action, @end,
                    ],
                ],
            ]
        }
    }
}
