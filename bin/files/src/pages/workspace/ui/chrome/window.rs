//! Resources and input state whose lifetime is the workspace window.

use super::*;

pub(in crate::pages::workspace) struct State {
    pub handle: AnyWindowHandle,
    pub tasks: Scope,
    pub desktop: desktop::Desktop,

    pub root_focus: FocusHandle,
    pub focus: FocusHandle,
    /// Held window-wide so tab close controls need no per-control timer.
    pub held_modifier: Option<ui::HeldModifier>,
    pub layout: responsive::Layout,
    pub subscriptions: Vec<Subscription>,
}

impl State {
    pub(in crate::pages::workspace) fn new(
        tasks: Scope,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            handle: window.window_handle(),
            tasks,
            desktop: desktop::Desktop::default(),
            root_focus: cx.focus_handle(),
            focus,
            held_modifier: None,
            layout: responsive::Layout::for_width(window.bounds().size.width),
            subscriptions: Vec::new(),
        }
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn subscribe(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity = cx.entity();
        let preferences = cx.gds().preferences().service();
        self.window.subscriptions = vec![
            cx.observe(&entity, |this, _, cx| this.sync_workspace_state(cx)),
            cx.observe_in(&preferences, window, |this, preferences, window, cx| {
                let current = preferences.read(cx).current().clone();
                this.sync_preferences(current, window, cx);
            }),
            cx.observe(&self.background_tasks, |_, _, cx| cx.notify()),
            cx.observe_window_bounds(window, Self::on_window_resize),
            cx.observe(&self.terminal.split, |_, _, cx| cx.notify()),
            cx.subscribe(&self.terminal.split, Self::on_terminal_split),
            cx.subscribe_in(&self.toolbar.path.input, window, Self::on_path_input),
            cx.subscribe_in(&self.toolbar.filter.input, window, Self::on_filter_input),
            cx.observe(&self.devices.service, Self::on_storage_changed),
            cx.observe(&self.sidebar.split, |_, _, cx| cx.notify()),
            cx.subscribe(&self.sidebar.split, Self::on_sidebar_split),
            // Refresh both the footer contents and cut markers on activation.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    cx.gds_mut().clipboard().refresh();
                } else {
                    // Releases outside this window are never delivered here.
                    cx.gds_mut().ui().keytips().release(window);
                    this.cancel_marquee(cx);
                    drag_drop::cancel_tab_hover(cx);
                    if this.window.held_modifier.take().is_some() {
                        cx.notify();
                    }
                }
            }),
        ];
    }
}
