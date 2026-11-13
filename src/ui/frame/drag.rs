use super::*;
use gds_uim::el;
use gpui_kit::IntoElement;

/// A draggable background. Controls above it claim or stop their mouse presses.
#[derive(Default, IntoElement)]
pub struct WindowDragArea;

impl RenderOnce for WindowDragArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        crate::ui::window::drag_bar_interactions(
            el! {
                .div {
                    # "toolbar-drag-area"; size_full;
                }
            },
            "toolbar-drag-area-drag-state",
            window,
            cx,
        )
    }
}
