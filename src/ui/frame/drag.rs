use super::*;
use gds_uim::el;

/// A draggable background. Controls above it claim or stop their mouse presses.
#[derive(Default, IntoElement)]
pub struct WindowDragArea;

impl RenderOnce for WindowDragArea {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        el! {
            .div {
                # "toolbar-drag-area"; size_full;
                on_mouse_down: MouseButton::Left, $|_, window, _| {
                    if !window.default_prevented() {
                        window.start_window_move();
                    }
                };
            }
        }
    }
}
