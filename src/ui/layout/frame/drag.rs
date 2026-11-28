use zgui::prelude::*;
use zgui::{component, view};

/// Draggable titlebar spacer initiating threshold window move, double-press maximize, and native
/// window menu when mounted outside [`super::Toolbar`] / [`crate::ui::window::TitleBar`].
///
/// Inside an enclosing top bar that already owns the drag controller, this renders as a pure
/// stretching spacer so presses bubble directly to the bar's shared gesture state.
#[component]
pub fn WindowDragArea(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    super::install_frame_stylesheets();
    if crate::ui::window::inside_window_drag_scope() {
        return AnyView::new(view! {
            row(
                class = "gds-window-drag-area",
                class = class,
                {..attrs}
            ) {
                {children.map(Children::into_view_once)}
            }
        });
    }
    let drag_ref = NodeRef::new();
    let window = zgui::prelude::try_use_window();
    let drag_handler = crate::ui::window::attach_window_drag(drag_ref, window);
    AnyView::new(view! {
        row(
            node_ref = drag_ref,
            class = "gds-window-drag-area",
            class = class,
            on:pointer_down = drag_handler,
            {..attrs}
        ) {
            {children.map(Children::into_view_once)}
        }
    })
}
