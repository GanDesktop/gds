use super::*;
use gds_uim::el;

/// The empty stretch beside the toolbar's leading control, which moves the
/// window.
///
/// The window has no native title bar, so this filler carries the interactive
/// move through the platform's own [`Window::start_window_move`]. It is a
/// sibling of the menu button and of the toolbar controls, never a wrapper, so
/// pressing those still only clicks them.
#[derive(Default, IntoElement)]
pub struct WindowDragArea;

impl RenderOnce for WindowDragArea {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        el! {
            .div {
                # "toolbar-drag-area"; flex_1; h_full;
                on_mouse_down: MouseButton::Left, $|_, window, _| window.start_window_move();
            }
        }
    }
}
