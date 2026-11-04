//! Explicit region focus is separate from the page and its selected contents.

use super::selection::Motion;
use super::*;

#[cfg(test)]
mod tests;

pub(super) struct State {
    pub tabs: FocusHandle,
    pub locations: FocusHandle,
    pub volumes: FocusHandle,
    pub tab: Option<u64>,
    pub location: LocationTarget,
    pub volume: Option<storage::VolumeId>,
    pub tabs_scroll: ScrollHandle,
    pub locations_scroll: ScrollHandle,
    pub volumes_scroll: ScrollHandle,
    /// Visiting the optional volume group reveals it for this session.
    pub reveal_volumes: bool,
}

impl State {
    pub fn new(cx: &mut App) -> Self {
        Self {
            tabs: cx.focus_handle(),
            locations: cx.focus_handle(),
            volumes: cx.focus_handle(),
            tab: None,
            location: LocationTarget::Home,
            volume: None,
            tabs_scroll: ScrollHandle::new(),
            locations_scroll: ScrollHandle::new(),
            volumes_scroll: ScrollHandle::new(),
            reveal_volumes: false,
        }
    }

    pub fn sidebar_focused(&self, window: &Window, cx: &App) -> bool {
        self.tabs.contains_focused(window, cx)
            || self.locations.contains_focused(window, cx)
            || self.volumes.contains_focused(window, cx)
    }
}

impl Browser {
    pub(super) fn focus_region(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pages.is_empty() {
            return;
        }

        self.toolbar.path.editing = false;
        match command {
            Command::FocusLocations => {
                self.scroll_region_location();
                self.sidebar.regions.locations.focus(window, cx);
            }
            Command::FocusTabs => {
                if self
                    .sidebar
                    .regions
                    .tab
                    .is_none_or(|id| !self.tabs.items.iter().any(|tab| tab.id == id))
                {
                    self.sidebar.regions.tab = self
                        .tabs
                        .items
                        .iter()
                        .find(|tab| tab.id == self.tabs.active_id)
                        .or(self.tabs.items.first())
                        .map(|tab| tab.id);
                }
                self.scroll_region_tab();
                self.sidebar.regions.tabs.focus(window, cx);
            }
            Command::FocusVolumes => {
                self.sidebar.regions.reveal_volumes = true;
                let ids = self.region_volumes(cx);
                if self
                    .sidebar
                    .regions
                    .volume
                    .as_ref()
                    .is_none_or(|id| !ids.contains(id))
                {
                    self.sidebar.regions.volume = ids.first().cloned();
                }
                self.sidebar.regions.volumes.focus(window, cx);
            }
            Command::FocusContent => self.window.focus.focus(window, cx),
            _ => unreachable!("only region commands are routed here"),
        }
        cx.notify();
    }

    fn region_volumes(&self, cx: &App) -> Vec<storage::VolumeId> {
        self.devices
            .service
            .read(cx)
            .snapshot
            .volumes
            .iter()
            .filter(|volume| volume.visible)
            .map(|volume| volume.id.clone())
            .collect()
    }

    fn scroll_region_location(&self) {
        if let Some(index) = LocationTarget::ALL
            .iter()
            .position(|target| *target == self.sidebar.regions.location)
        {
            self.sidebar.regions.locations_scroll.scroll_to_item(index);
        }
    }

    fn scroll_region_tab(&self) {
        if let Some(index) = self
            .tabs
            .items
            .iter()
            .position(|tab| Some(tab.id) == self.sidebar.regions.tab)
        {
            self.sidebar.regions.tabs_scroll.scroll_to_item(index);
        }
    }

    pub(super) fn on_region_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sidebar.regions.sidebar_focused(window, cx)
            || !self.pages.is_empty()
            || self.menus.open.is_some()
            || self.menus.context.is_some()
            || self.menus.sort.is_some()
            || window.has_active_dialog(cx)
            || self.operations.rename.read(cx).is_open()
            || event.keystroke.modifiers.modified()
        {
            return;
        }
        let Some(motion) = Motion::for_key(&event.keystroke.key) else {
            return;
        };
        if self.sidebar.regions.locations.contains_focused(window, cx) {
            let current = LocationTarget::ALL
                .iter()
                .position(|target| *target == self.sidebar.regions.location);
            if let Some(index) = motion.target(current, LocationTarget::ALL.len()) {
                self.sidebar.regions.location = LocationTarget::ALL[index];
                self.scroll_region_location();
                self.sidebar.regions.locations.focus(window, cx);
            }
        } else if self.sidebar.regions.tabs.contains_focused(window, cx) {
            let current = self
                .tabs
                .items
                .iter()
                .position(|tab| Some(tab.id) == self.sidebar.regions.tab);
            if let Some(index) = motion.target(current, self.tabs.items.len()) {
                self.sidebar.regions.tab = Some(self.tabs.items[index].id);
                self.scroll_region_tab();
                self.sidebar.regions.tabs.focus(window, cx);
            }
        } else if self.sidebar.regions.volumes.contains_focused(window, cx) {
            let ids = self.region_volumes(cx);
            let current = ids
                .iter()
                .position(|id| Some(id) == self.sidebar.regions.volume.as_ref());
            if let Some(index) = motion.target(current, ids.len()) {
                self.sidebar.regions.volume = Some(ids[index].clone());
                self.sidebar.regions.volumes_scroll.scroll_to_item(index);
                self.sidebar.regions.volumes.focus(window, cx);
            }
        }
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }

    pub(super) fn activate_region(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar.regions.locations.is_focused(window) {
            self.show_location(
                self.sidebar.regions.location.location(&self.locations.home),
                window,
                cx,
            );
        } else if self.sidebar.regions.tabs.is_focused(window) {
            if let Some(id) = self.sidebar.regions.tab {
                self.switch_tab(id, window, cx);
            }
        } else if self.sidebar.regions.volumes.is_focused(window)
            && let Some(id) = self.sidebar.regions.volume.clone()
        {
            self.show_volume(id, window, cx);
        }
    }

    pub(super) fn on_focus_locations(
        &mut self,
        _: &FocusLocations,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::FocusLocations, window, cx);
    }
    pub(super) fn on_focus_volumes(
        &mut self,
        _: &FocusVolumes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::FocusVolumes, window, cx);
    }
    pub(super) fn on_focus_tabs(
        &mut self,
        _: &FocusTabs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::FocusTabs, window, cx);
    }
    pub(super) fn on_focus_content(
        &mut self,
        _: &FocusContent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::FocusContent, window, cx);
    }
    pub(super) fn on_focus_notifications(
        &mut self,
        _: &FocusNotifications,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::FocusNotifications, window, cx);
    }
    pub(super) fn on_close_notification(
        &mut self,
        _: &CloseNotification,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::CloseNotification, window, cx);
    }
    pub(super) fn on_close_all_notifications(
        &mut self,
        _: &CloseAllNotifications,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(Command::CloseAllNotifications, window, cx);
    }
}
