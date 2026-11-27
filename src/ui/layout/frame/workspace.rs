use zgui::prelude::*;
use zgui::{component, view};

use super::{FrameContext, vertical_divider};

/// Flexible workspace body region inside [`super::Frame`].
///
/// Set `paint_content_surface = false` when wrapping composite split panes (such as a top file
/// browser + bottom terminal dock) that each paint their own translucent material once.
#[component]
pub fn WorkspaceFrame(
    #[prop(optional)] sidebar: Option<AnyView>,
    #[prop(optional)] content: Option<AnyView>,
    #[prop(default = true)] paint_content_surface: bool,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    super::install_frame_stylesheets();

    if sidebar.is_none() && content.is_none() {
        return AnyView::new(view! {
            row(
                class = "gds-workspace-frame gds-workspace-frame--legacy",
                attr:data-paint-surface = paint_content_surface.then(|| "true".to_owned()),
                class = class,
                {..attrs}
            ) {
                {children.map(Children::into_view_once)}
            }
        });
    }

    let sidebar_width = FrameContext::current().and_then(|ctx| ctx.sidebar_width);
    let divider = vertical_divider(sidebar_width);

    let sidebar_col = sidebar_width.map(|width_sig| {
        AnyView::new(view! {
            box(
                class = "gds-workspace-frame__sidebar",
                style:width = move || Some(format!("{:.2}px", width_sig.get()))
            ) {
                {sidebar}
            }
        })
    });

    AnyView::new(view! {
        row(class = "gds-workspace-frame", class = class, {..attrs}) {
            {sidebar_col}
            box(
                class = "gds-workspace-frame__content",
                attr:data-paint-surface = paint_content_surface.then(|| "true".to_owned())
            ) {
                {content}
                {children.map(Children::into_view_once)}
            }
            {divider}
        }
    })
}
