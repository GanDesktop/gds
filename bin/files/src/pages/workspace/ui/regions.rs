//! Explicit region focus is separate from the page and its selected contents.

use super::selection::Motion;
use super::*;

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
        if self.settings_open {
            return;
        }
        self.reset_tab_prefix();
        self.path_editing = false;
        match command {
            Command::FocusLocations => {
                self.scroll_region_location();
                self.regions.locations.focus(window, cx);
            }
            Command::FocusTabs => {
                if self
                    .regions
                    .tab
                    .is_none_or(|id| !self.tabs.iter().any(|tab| tab.id == id))
                {
                    self.regions.tab = self
                        .tabs
                        .iter()
                        .find(|tab| tab.id == self.active_id)
                        .or(self.tabs.first())
                        .map(|tab| tab.id);
                }
                self.scroll_region_tab();
                self.regions.tabs.focus(window, cx);
            }
            Command::FocusVolumes => {
                self.regions.reveal_volumes = true;
                let ids = self.region_volumes(cx);
                if self
                    .regions
                    .volume
                    .as_ref()
                    .is_none_or(|id| !ids.contains(id))
                {
                    self.regions.volume = ids.first().cloned();
                }
                self.regions.volumes.focus(window, cx);
            }
            Command::FocusContent => self.focus.focus(window, cx),
            _ => unreachable!("only region commands are routed here"),
        }
        cx.notify();
    }

    fn region_volumes(&self, cx: &App) -> Vec<storage::VolumeId> {
        self.storage
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
            .position(|target| *target == self.regions.location)
        {
            self.regions.locations_scroll.scroll_to_item(index);
        }
    }

    fn scroll_region_tab(&self) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| Some(tab.id) == self.regions.tab)
        {
            self.regions.tabs_scroll.scroll_to_item(index);
        }
    }

    pub(super) fn on_region_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.regions.sidebar_focused(window, cx)
            || self.settings_open
            || self.menu_open
            || self.keyboard_menu.is_some()
            || self.sort_menu.is_some()
            || window.has_active_dialog(cx)
            || self.rename.read(cx).is_open()
            || event.keystroke.modifiers.modified()
        {
            return;
        }
        let Some(motion) = Motion::for_key(&event.keystroke.key) else {
            return;
        };
        if self.regions.locations.is_focused(window) {
            let current = LocationTarget::ALL
                .iter()
                .position(|target| *target == self.regions.location);
            if let Some(index) = motion.target(current, LocationTarget::ALL.len()) {
                self.regions.location = LocationTarget::ALL[index];
                self.scroll_region_location();
            }
        } else if self.regions.tabs.is_focused(window) {
            let current = self
                .tabs
                .iter()
                .position(|tab| Some(tab.id) == self.regions.tab);
            if let Some(index) = motion.target(current, self.tabs.len()) {
                self.regions.tab = Some(self.tabs[index].id);
                self.scroll_region_tab();
            }
        } else if self.regions.volumes.is_focused(window) {
            let ids = self.region_volumes(cx);
            let current = ids
                .iter()
                .position(|id| Some(id) == self.regions.volume.as_ref());
            if let Some(index) = motion.target(current, ids.len()) {
                self.regions.volume = Some(ids[index].clone());
                self.regions.volumes_scroll.scroll_to_item(index);
            }
        }
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }

    pub(super) fn activate_region(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.regions.locations.is_focused(window) {
            self.show_location(self.regions.location.location(&self.home), window, cx);
        } else if self.regions.tabs.is_focused(window) {
            if let Some(id) = self.regions.tab {
                self.switch_tab(id, window, cx);
            }
        } else if self.regions.volumes.is_focused(window)
            && let Some(id) = self.regions.volume.clone()
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
