use gpui_kit::{prelude::*, *};

/// A dialog control's own focus node, so activating the control dispatches its
/// action on the dialog it belongs to rather than on whatever holds focus.
///
/// `Window::dispatch_action` routes along the focused element; dispatching from
/// a node inside the control walks the control's own ancestors, which end at
/// the dialog that owns it, stacked dialogs included.
#[derive(Clone)]
pub(super) struct DispatchAnchor {
    handle: FocusHandle,
}

impl DispatchAnchor {
    pub(super) fn new(key: &'static str, window: &mut Window, cx: &mut App) -> Self {
        let handle = window
            .use_keyed_state(key, cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        Self { handle }
    }

    /// Zero-size, out of flow and never hovered: it only gives the handle a
    /// place in the dispatch tree, not in the tab order.
    pub(super) fn element(&self) -> Div {
        div().absolute().size_0().track_focus(&self.handle)
    }

    pub(super) fn dispatch(&self, action: &dyn Action, window: &mut Window, cx: &mut App) {
        if self.handle.is_focused(window) || self.handle.contains(&self.handle, window) {
            self.handle.dispatch_action(action, window, cx);
        } else {
            window.dispatch_action(action.boxed_clone(), cx);
        }
    }
}
