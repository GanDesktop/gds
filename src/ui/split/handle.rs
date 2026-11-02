use gds_uim::{component, el};
use gpui_kit::{IntoElement, prelude::*, *};

use super::{DIVIDER_HEIGHT, KEY_CONTEXT, KEY_STEP, MoveDown, MoveUp, State};

#[derive(Clone)]
pub(super) struct Resize {
    pub(super) state: Entity<State>,
}

impl Render for Resize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[component]
#[derive(IntoElement)]
pub(super) struct SplitHandle {
    state: Entity<State>,
    #[attr(into)]
    label: SharedString,
}

impl SplitHandle {
    pub(super) fn new(state: Entity<State>) -> Self {
        Self {
            state,
            label: SharedString::default(),
        }
    }
}

impl RenderOnce for SplitHandle {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = self.state.read(cx).focus.clone();
        let press_focus = focus.clone();
        let up = self.state.clone();
        let down = self.state.clone();
        let handle = gpui_kit::component::resizable::resize_handle::<Resize, Resize>(
            "vertical-split-divider",
            Axis::Vertical,
        )
        .track_focus(&focus)
        .key_context(KEY_CONTEXT)
        .tab_index(0)
        .role(Role::Splitter)
        .aria_label(self.label.clone())
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(self.label.clone()).build(window, cx)
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            press_focus.focus(window, cx);
            cx.stop_propagation();
        })
        .on_drag(Resize { state: self.state }, |drag, _, window, cx| {
            drag.state.update(cx, |state, cx| {
                state.dragging = true;
                state.focus.focus(window, cx);
                cx.notify();
            });
            cx.new(|_| drag.clone())
        })
        .on_action(move |_: &MoveUp, _, cx| {
            up.update(cx, |state, cx| {
                state.adjust(state.height() - KEY_STEP, cx);
                state.commit(cx);
            });
            cx.stop_propagation();
        })
        .on_action(move |_: &MoveDown, _, cx| {
            down.update(cx, |state, cx| {
                state.adjust(state.height() + KEY_STEP, cx);
                state.commit(cx);
            });
            cx.stop_propagation();
        });
        el! {
            .div { absolute; left_0; right_0; top: $px((DIVIDER_HEIGHT - 1.) / 2.); h: 1.0.px } [handle]
        }
    }
}
