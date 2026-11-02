//! Resources and input state whose lifetime is the workspace window.

use super::*;

pub(super) struct State {
    pub handle: AnyWindowHandle,
    pub runtime: Handle,
    pub desktop: desktop::Desktop,

    pub root_focus: FocusHandle,
    pub focus: FocusHandle,
    /// Held window-wide so tab close controls need no per-control timer.
    pub held_modifier: Option<ui::HeldModifier>,
    pub subscriptions: Vec<Subscription>,
}

impl State {
    pub(super) fn new(runtime: Handle, window: &mut Window, cx: &mut App) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            handle: window.window_handle(),
            runtime,
            desktop: desktop::Desktop::default(),
            root_focus: cx.focus_handle(),
            focus,
            held_modifier: None,
            subscriptions: Vec::new(),
        }
    }
}

impl Browser {
    pub(super) fn subscribe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.window.subscriptions = vec![
            cx.observe(&self.terminal.split, |_, _, cx| cx.notify()),
            cx.subscribe(&self.terminal.split, Self::on_terminal_split),
            cx.subscribe_in(&self.toolbar.path.input, window, Self::on_path_input),
            cx.subscribe_in(&self.toolbar.filter.input, window, Self::on_filter_input),
            cx.subscribe_in(&self.settings.view, window, Self::on_settings_event),
            cx.observe(&self.devices.service, Self::on_storage_changed),
            cx.observe(&self.sidebar.split, |_, _, cx| cx.notify()),
            cx.subscribe(&self.sidebar.split, Self::on_sidebar_split),
            // Refresh both the footer contents and cut markers on activation.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    file_clipboard::refresh(cx);
                } else {
                    // Releases outside this window are never delivered here.
                    ui::keytips::release(window, cx);
                    this.cancel_marquee(cx);
                    drag_drop::cancel_tab_hover(cx);
                    this.tabs.prefix = None;
                    if this.window.held_modifier.take().is_some() {
                        cx.notify();
                    }
                }
            }),
        ];
    }
}
