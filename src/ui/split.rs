//! A vertical split whose preferred height survives temporary space constraints.

mod handle;
mod state;

use gds_uim::{component, el};
use gpui_kit::component::StyledExt as _;
use gpui_kit::{prelude::*, *};

use handle::{Resize, SplitHandle};
pub use state::{State, constrained_height};

#[cfg(test)]
mod tests;

pub const DIVIDER_HEIGHT: f32 = 9.;
const KEY_STEP: f32 = 12.;
const KEY_CONTEXT: &str = "GdsVerticalSplit";
actions!(gds_split, [MoveUp, MoveDown]);

#[derive(Clone, Copy)]
pub struct Changed(pub f32);

/// The application supplies persistent split state; panes and labels are composable content.
#[component(gpui = gpui_kit)]
#[derive(IntoElement)]
pub struct VerticalSplit {
    state: Entity<State>,
    #[attr(any_element)]
    top: AnyElement,
    #[attr(any_element)]
    bottom: AnyElement,
    #[attr(into)]
    label: SharedString,
    #[attr(any_element, option)]
    divider_background: Option<AnyElement>,
    style: StyleRefinement,
}

impl VerticalSplit {
    pub fn new(state: Entity<State>) -> Self {
        Self {
            state,
            top: div().into_any_element(),
            bottom: div().into_any_element(),
            label: SharedString::default(),
            divider_background: None,
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for VerticalSplit {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for VerticalSplit {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let height = state.height();
        let bounds = state.bounds.clone();
        let measure_state = self.state.downgrade();
        let release_state = self.state.downgrade();
        let measure = canvas(
            move |new_bounds, _, cx| {
                if bounds.replace(new_bounds) != new_bounds {
                    let _ = measure_state.update(cx, |_, cx| cx.notify());
                }
            },
            move |_, _, window, _| {
                // ResizeHandle captures the drag. Persistence also needs release outside the split.
                window.on_mouse_event({
                    let release = release_state.clone();
                    move |event: &MouseUpEvent, phase, _, cx| {
                        if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                            return;
                        }
                        let _ = release.update(cx, |state, cx| {
                            if state.dragging {
                                state.dragging = false;
                                state.commit(cx);
                                cx.notify();
                            }
                        });
                    }
                });
                window.on_mouse_event({
                    let release = release_state.clone();
                    move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase != DispatchPhase::Capture
                            || event.pressed_button == Some(MouseButton::Left)
                        {
                            return;
                        }
                        let _ = release.update(cx, |state, cx| {
                            if state.dragging {
                                state.dragging = false;
                                state.commit(cx);
                                cx.notify();
                            }
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full();
        let dragged_state = self.state.clone();
        let divider = el! {
            .div { relative; flex_shrink_0; w_full; h: DIVIDER_HEIGHT.px } [
                ..$(self.divider_background),
                $(SplitHandle::new(self.state.clone()).label(self.label)),
            ]
        };
        el! {
            .div { # "vertical-split"; relative; flex; flex_col; size_full; min_h_0; overflow_hidden;
                refine_style: $&self.style;
                on_drag_move: $(move |event: &DragMoveEvent<Resize>, _, cx| {
                    if event.drag(cx).state != dragged_state {
                        return;
                    }
                    dragged_state.update(cx, |state, cx| {
                        state.adjust(f32::from(event.event.position.y - state.bounds.get().origin.y), cx);
                    });
                });
            } [
                .div { flex; flex_col; flex_shrink_0; h: height.px; min_h_0; overflow_hidden } [$(self.top)],
                divider,
                .div { flex; flex_col; flex_1; min_h_0; overflow_hidden } [$(self.bottom)],
                measure,
            ]
        }
    }
}
