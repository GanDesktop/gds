use zgui::prelude::*;
use zgui::{component, view};

use super::{FrameContext, MENU_PADDING, TOOLBAR_HEIGHT, vertical_divider};
use crate::appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, ICON_BUTTON_SIZE, SIDEBAR_WIDTH};
use crate::ui::window::WindowControlsProps;

/// Fixed-height window top toolbar aligned to [`super::Frame`]'s single-column split.
#[component]
pub fn Toolbar(
    #[prop(optional)] left: Option<AnyView>,
    #[prop(optional)] menu: Option<AnyView>,
    #[prop(optional)] controls: Option<AnyView>,
    #[prop(optional)] window_controls: Option<AnyView>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    super::install_frame_stylesheets();
    crate::ui::window::provide_window_drag_scope();
    let toolbar_ref = NodeRef::new();
    let window = zgui::prelude::try_use_window();
    let drag_handler = crate::ui::window::attach_window_drag(toolbar_ref, window);

    if left.is_none() && menu.is_none() && controls.is_none() && window_controls.is_none() {
        return AnyView::new(view! {
            row(
                node_ref = toolbar_ref,
                class = "gds-toolbar gds-toolbar--legacy",
                style:height = format!("{TOOLBAR_HEIGHT}px"),
                on:pointer_down = drag_handler,
                class = class,
                {..attrs}
            ) {
                {children.map(Children::into_view_once)}
            }
        });
    }

    let sidebar_width = FrameContext::current().and_then(|ctx| ctx.sidebar_width);
    let has_sidebar = sidebar_width.is_some();
    let fallback_width = 2.0 * MENU_PADDING + ICON_BUTTON_SIZE;
    let collapsed_inset = (COLLAPSED_SIDEBAR_WIDTH - ICON_BUTTON_SIZE) * 0.5;
    let span = SIDEBAR_WIDTH - COLLAPSED_SIDEBAR_WIDTH;

    let win_controls = window_controls.unwrap_or_else(|| AnyView::new(view! { WindowControls() }));
    let divider = vertical_divider(sidebar_width);

    let left_class = if has_sidebar {
        "gds-toolbar__left gds-toolbar__left--sidebar"
    } else {
        "gds-toolbar__left gds-toolbar__left--content"
    };

    let left_body = if let Some(leading) = left {
        leading
    } else {
        AnyView::new(view! {
            box(class = "gds-toolbar__menu") {
                {menu}
            }
        })
    };

    AnyView::new(view! {
        row(
            node_ref = toolbar_ref,
            class = "gds-toolbar",
            style:height = format!("{TOOLBAR_HEIGHT}px"),
            on:pointer_down = drag_handler,
            class = class,
            {..attrs}
        ) {
            row(
                class = left_class,
                style:width = move || {
                    let w = sidebar_width.map_or(fallback_width, |sig| sig.get());
                    Some(format!("{w:.2}px"))
                },
                style:padding-left = move || {
                    let inset = match sidebar_width {
                        Some(sig) if span.abs() > f32::EPSILON => {
                            let progress = ((sig.get() - COLLAPSED_SIDEBAR_WIDTH) / span).clamp(0.0, 1.0);
                            collapsed_inset + (MENU_PADDING - collapsed_inset) * progress
                        }
                        _ => MENU_PADDING,
                    };
                    Some(format!("{inset:.2}px"))
                },
                style:padding-right = format!("{MENU_PADDING}px")
            ) {
                {left_body}
            }
            row(class = "gds-toolbar__right") {
                row(class = "gds-toolbar__controls") {
                    {controls}
                    {children.map(Children::into_view_once)}
                }
                {win_controls}
            }
            {divider}
        }
    })
}
