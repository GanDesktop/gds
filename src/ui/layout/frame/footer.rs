use zgui::prelude::*;
use zgui::{component, view};

use super::{FOOTER_HEIGHT, FrameContext, STATUS_PADDING, vertical_divider};

/// Fixed-height window footer aligned to [`super::Frame`]'s single-column split, drawing its
/// horizontal `1px` top border exclusively on the right content column starting at `sidebar_width`.
#[component]
pub fn Footer(
    #[prop(optional)] sidebar_status: Option<AnyView>,
    #[prop(optional)] status: Option<AnyView>,
    #[prop(optional)] controls: Option<AnyView>,
    #[prop(optional)] extras: Option<AnyView>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    super::install_frame_stylesheets();

    if sidebar_status.is_none() && status.is_none() && controls.is_none() && extras.is_none() {
        return AnyView::new(view! {
            row(
                class = "gds-footer gds-footer--legacy",
                style:height = format!("{FOOTER_HEIGHT}px"),
                class = class,
                {..attrs}
            ) {
                {children.map(Children::into_view_once)}
            }
        });
    }

    let sidebar_width = FrameContext::current().and_then(|ctx| ctx.sidebar_width);
    let divider = vertical_divider(sidebar_width);

    let left_col = sidebar_width.map(|width_sig| {
        AnyView::new(view! {
            row(
                class = "gds-footer__left",
                style:width = move || Some(format!("{:.2}px", width_sig.get()))
            ) {
                {sidebar_status}
            }
        })
    });

    AnyView::new(view! {
        row(
            class = "gds-footer",
            style:height = format!("{FOOTER_HEIGHT}px"),
            class = class,
            {..attrs}
        ) {
            {left_col}
            row(
                class = "gds-footer__right",
                style:padding-left = format!("{STATUS_PADDING}px"),
                style:padding-right = format!("{STATUS_PADDING}px")
            ) {
                row(class = "gds-footer__status") {
                    {status}
                    {extras}
                    {children.map(Children::into_view_once)}
                }
                {controls.map(|ctrl| AnyView::new(view! { row(class = "gds-footer__controls") {{ctrl}} }))}
            }
            {divider}
        }
    })
}
