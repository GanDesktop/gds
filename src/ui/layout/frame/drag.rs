use zgui::prelude::*;
use zgui::{component, view};

/// Titlebar region initiating compositor window move/maximize on primary press and native window
/// menu on secondary press while ignoring presses inside focusable child controls.
#[component]
pub fn WindowDragArea(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    super::install_frame_stylesheets();
    let window = zgui::prelude::try_use_window();
    let drag_handler = crate::ui::window::drag_pointer_down_handler(window);
    view! {
        row(
            class = "gds-window-drag-area",
            class = class,
            on:pointer_down = drag_handler,
            {..attrs}
        ) {
            {children.map(Children::into_view_once)}
        }
    }
}
