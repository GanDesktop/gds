//! Shared focus presentation for keyboard-accessible window regions.

use gpui_kit::component::ActiveTheme;
use gpui_kit::{prelude::*, *};

use crate::ui::{Shortcut, keytips};

/// A region wrapper with its own focus target and the shared Alt key hint.
/// Callers supply layout and contents; focus changes do not change its geometry.
pub fn surface(
    id: &'static str,
    focus: &FocusHandle,
    command: impl Into<Shortcut>,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let active = focus.contains_focused(window, cx);
    let hint = keytips::anchor(window, cx, format!("{id}-hint").into(), command.into());
    div()
        .id(id)
        .relative()
        .min_w_0()
        .min_h_0()
        .track_focus(focus)
        .border_1()
        .border_color(if active {
            cx.theme().ring
        } else {
            transparent_black()
        })
        .child(hint)
}

/// An inset focus marker shared by regions and their keyboard cursor rows.
pub fn focus_ring(cx: &App) -> Div {
    div()
        .absolute()
        .inset_0()
        .border_1()
        .border_color(cx.theme().ring)
        .rounded(px(4.))
}
