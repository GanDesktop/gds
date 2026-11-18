//! Files command routing follows focus boundaries; native editors retain editing keys.
use super::{
    model::{Pane, Prompt, Row},
    ui::Browser,
};
use crate::{
    entities::{
        file::ViewMode,
        location::{Collection, LocationTarget, volumes},
    },
    shared::ui::actions::Command,
};
use anti_webui::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SidebarRegion {
    Tabs,
    Locations,
    Volumes,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum SidebarRow {
    Tab(u64),
    Location(LocationTarget),
    Volume(volumes::VolumeId),
}

impl Browser {
    pub(super) fn dispatch(&self, command: Command) {
        use Command::*;
        if self.pane() == Pane::Files
            && matches!(
                command,
                Open | NewFolder
                    | OpenInNewTab
                    | Copy
                    | Cut
                    | Paste
                    | Rename
                    | Trash
                    | DeletePermanent
                    | SelectAll
                    | DeselectAll
            )
            && self.read(|state| state.tab().navigation.is_loading())
        {
            return;
        }
        self.0.owner.with(|| match command {
            Menu => self.show_app_menu(),
            Settings => self.toggle_settings(),
            Help => self.show_help(),
            NewFolder => self.new_folder(),
            NewTab => self.new_tab(None),
            NewWindow => {
                let path = self.read(|state| {
                    state
                        .tab()
                        .navigation
                        .path()
                        .unwrap_or(&state.home)
                        .to_owned()
                });
                (self.0.new_window)(path);
            }
            CloseTab => {
                let active = self.read(|state| state.active);
                self.close_tab(active);
            }
            NextTab | PreviousTab => {
                if self.pane() == Pane::Settings {
                    self.0.settings.cycle_page(command == PreviousTab);
                    return;
                }
                let next = self.read(|state| {
                    if command == NextTab {
                        (state.active + 1) % state.tabs.len()
                    } else {
                        (state.active + state.tabs.len() - 1) % state.tabs.len()
                    }
                });
                self.select_tab(next);
            }
            Sidebar => self.0.sidebar.update(|visible| *visible = !*visible),
            Terminal => self.toggle_terminal(),
            EditLocation => self.start_address_edit(),
            Back => self.history(false),
            Forward => self.history(true),
            Parent => self.parent(),
            Refresh => self.refresh(),
            Filter => {
                if !self.filter_available() {
                    return;
                }
                self.0.filter_node.focus();
                let value = self.0.filter.get_untracked();
                self.0.filter_node.set_text(&value, (0, value.len()));
            }
            Hidden => self.edit_preferences(|preferences| {
                preferences.files.show_hidden = !preferences.files.show_hidden
            }),
            Favorite => self.favorite(),
            Open => self.open_selected(),
            OpenInNewTab => self.open_selected_tab(),
            Copy => self.copy(false),
            Cut => self.copy(true),
            Paste => self.paste(),
            Undo => self.replay(false),
            Redo => self.replay(true),
            CopyPaths => self.copy_selected_paths(),
            SelectAll => self.select_all(),
            DeselectAll => self
                .update_selection(|state| state.tab_mut().navigation.view_mut().selection.clear()),
            ContextMenu => self.show_context_menu(),
            Rename => self.rename(),
            Details => self.toggle_details(),
            Columns => self.configure_columns(),
            ViewMode => self.toggle_view(),
            Sort => self.show_sort_menu(),
            Trash => self.delete(false),
            DeletePermanent => self.delete(true),
            RestoreSelection => self.restore_trash(false, false),
            PurgeSelection => self.restore_trash(true, false),
            EmptyTrash => self.restore_trash(true, true),
            ClearRecent => {
                self.update(|state| state.places.clear_recent());
                self.save_places();
                let ids = self.read(|state| {
                    state
                        .tabs
                        .iter()
                        .filter(|tab| {
                            matches!(
                                tab.navigation.location(),
                                crate::entities::location::Location::Recent
                            )
                        })
                        .map(|tab| tab.id)
                        .collect::<Vec<_>>()
                });
                for id in ids {
                    self.refresh_tab(id);
                }
            }
            MountVolume | UnmountVolume => {
                let volume = self.read(|state| {
                    state
                        .tab()
                        .selected()
                        .into_iter()
                        .find_map(|row| match row {
                            Row::Volume(volume) => Some(volume),
                            _ => None,
                        })
                });
                if let Some(volume) = volume {
                    if command == MountVolume {
                        self.change_volume(volumes::DeviceAction::Mount(volume.id));
                    } else {
                        self.change_volume(volumes::DeviceAction::Unmount(volume.id));
                    }
                }
            }
            FocusContent => self.focus_content(),
            FocusTabs => self.focus_sidebar(SidebarRegion::Tabs),
            FocusLocations => self.focus_sidebar(SidebarRegion::Locations),
            FocusVolumes => self.focus_sidebar(SidebarRegion::Volumes),
            FocusNotifications => {
                self.0
                    .notifications
                    .focus_current(if self.pane() == Pane::Files {
                        self.0.content_focus
                    } else {
                        self.0.settings_node
                    })
            }
            CloseNotification => self.0.notifications.dismiss_current(),
            CloseAllNotifications => {
                self.0.notifications.clear();
                self.0.notice.set(None);
            }
            Cancel => self.cancel_current(),
        });
    }
    fn focus_sidebar(&self, region: SidebarRegion) {
        if self.pane() != Pane::Files {
            return;
        }
        let node = self.sidebar_node(region);
        let current = match region {
            SidebarRegion::Tabs => Some(self.read(|state| SidebarRow::Tab(state.tab().id))),
            SidebarRegion::Locations => Some(SidebarRow::Location(LocationTarget::Home)),
            SidebarRegion::Volumes => {
                self.0.sidebar_reveal_volumes.set_if_changed(true);
                self.sidebar_rows(region).into_iter().next()
            }
        };
        self.0.sidebar_region.set_if_changed(Some(region));
        self.0.sidebar_cursor.set_if_changed(current);
        request_frame(move |_| node.focus());
    }

    pub(super) fn sidebar_node(&self, region: SidebarRegion) -> NodeRef {
        match region {
            SidebarRegion::Tabs => self.0.tabs_node,
            SidebarRegion::Locations => self.0.locations_node,
            SidebarRegion::Volumes => self.0.volumes_node,
        }
    }

    pub(super) fn sidebar_location_targets(&self) -> impl Iterator<Item = LocationTarget> + use<> {
        let show_tags = self.0.preferences.snapshot().tags.enabled;
        LocationTarget::ALL.into_iter().filter(move |target| {
            !matches!(target, LocationTarget::Collection(Collection::Tags)) || show_tags
        })
    }

    fn sidebar_rows(&self, region: SidebarRegion) -> Vec<SidebarRow> {
        match region {
            SidebarRegion::Tabs => self.read(|state| {
                state
                    .tabs
                    .iter()
                    .map(|tab| SidebarRow::Tab(tab.id))
                    .collect()
            }),
            SidebarRegion::Locations => self
                .sidebar_location_targets()
                .map(SidebarRow::Location)
                .collect(),
            SidebarRegion::Volumes => self
                .0
                .volumes
                .snapshot()
                .volumes
                .into_iter()
                .filter(|volume| volume.visible)
                .map(|volume| SidebarRow::Volume(volume.id))
                .collect(),
        }
    }

    pub(super) fn sidebar_focus_changed(&self, region: SidebarRegion, focused: bool) {
        if !focused && self.0.sidebar_region.get_untracked() == Some(region) {
            self.0.sidebar_region.set_if_changed(None);
            self.0.sidebar_cursor.set_if_changed(None);
        }
    }

    pub(super) fn sidebar_key(&self, event: &KeyboardEvent, region: SidebarRegion) {
        if event.modifiers != Modifiers::default() || self.0.menus.is_open() {
            return;
        }
        let cursor = self.0.sidebar_cursor.get_untracked();
        if event.key == Key::Named(NamedKey::Enter) {
            if self.0.sidebar_region.get_untracked() != Some(region) {
                return;
            }
            match cursor {
                Some(SidebarRow::Tab(id)) => {
                    if let Some(index) = self.tab_index(id) {
                        self.select_tab(index);
                    }
                }
                Some(SidebarRow::Location(target)) => {
                    let location = self.read(|state| target.location(&state.home));
                    self.open(location);
                }
                Some(SidebarRow::Volume(id)) => {
                    self.open_volume(&id, false);
                }
                None => return,
            }
            self.focus_content_after_mount();
        } else {
            let rows = self.sidebar_rows(region);
            let Some(last) = rows.len().checked_sub(1) else {
                return;
            };
            let current = cursor.and_then(|cursor| rows.iter().position(|row| row == &cursor));
            let index = match event.key {
                Key::Named(NamedKey::ArrowDown) => current.map_or(0, |index| (index + 1).min(last)),
                Key::Named(NamedKey::ArrowUp) => {
                    current.map_or(last, |index| index.saturating_sub(1))
                }
                Key::Named(NamedKey::Home) => 0,
                Key::Named(NamedKey::End) => last,
                _ => return,
            };
            let node = self.sidebar_node(region);
            self.0.sidebar_region.set_if_changed(Some(region));
            self.0
                .sidebar_cursor
                .set_if_changed(Some(rows[index].clone()));
            node.focus();
            let row_height = if region == SidebarRegion::Tabs {
                gds::app::Gds::current()
                    .appearance()
                    .typography()
                    .scaled(44.0)
            } else {
                gds::appearance::tokens::SIDEBAR_ITEM_HEIGHT
            };
            scroll_row_into_view(node, index, row_height + 4.0);
        }
        event.prevent_default();
        event.stop_propagation();
    }

    fn cancel_current(&self) {
        if self.pane() != Pane::Files {
            self.back_page();
            return;
        }
        if !matches!(self.0.prompt.get_untracked(), Prompt::Closed) {
            self.cancel_prompt();
            return;
        }
        if self.read(|state| state.operation.busy()) {
            self.read(|state| state.operation.cancel());
            return;
        }
        if self.read(|state| state.tab().navigation.is_loading()) {
            self.cancel_location_load();
            return;
        }
        if let Some(task) = crate::shared::lib::clipboard::Clipboard::current().cancel_cut() {
            let weak = self.downgrade();
            task.then(move |result| {
                if let (Some(browser), Err(error)) = (weak.upgrade(), result) {
                    browser.report_notice(error.to_string());
                }
            });
            return;
        }
        if !self.0.filter.get_untracked().is_empty() {
            self.0.filter.set(String::new());
            self.filter(String::new());
            self.focus_content();
            return;
        }
        if self.read(|state| !state.tab().navigation.view().selection.is_empty()) {
            self.update_selection(|state| state.tab_mut().navigation.view_mut().selection.clear());
            self.focus_content();
            return;
        }
        if self.read(|state| state.picker.is_some()) {
            self.cancel_picker();
            self.0.window.close();
        }
    }
    pub(super) fn content_key(&self, event: &KeyboardEvent) {
        if self.menu_key(event) {
            return;
        }
        if self.picker_key(event) {
            event.prevent_default();
            event.stop_propagation();
            return;
        }
        if let Some(command) = Command::from_key(event) {
            if command.needs_content_focus() {
                self.dispatch(command);
                event.prevent_default();
                event.stop_propagation();
            }
            return;
        }
        if event.key == Key::Named(NamedKey::Backspace) {
            self.parent();
            event.prevent_default();
            event.stop_propagation();
            return;
        }
        let columns = self.grid_columns();
        let height = self.0.list.logical_rect().map_or(400.0, |rect| rect.height);
        let target = self.read(|state| {
            let tab = state.tab();
            let count = tab.rows.len();
            if count == 0 {
                return None;
            }
            let current = tab
                .navigation
                .view()
                .cursor
                .as_ref()
                .and_then(|cursor| tab.rows.iter().position(|row| row.key() == *cursor));
            let grid = tab.mode == ViewMode::Grid;
            let stride = if grid { columns } else { 1 };
            let page = ((height / self.row_height()).floor().max(1.0) as usize) * stride;
            match event.key {
                Key::Named(NamedKey::ArrowDown) => {
                    Some(current.map_or(0, |index| (index + stride).min(count - 1)))
                }
                Key::Named(NamedKey::ArrowUp) => {
                    Some(current.map_or(count - 1, |index| index.saturating_sub(stride)))
                }
                Key::Named(NamedKey::ArrowRight) if grid => {
                    Some(current.map_or(0, |index| (index + 1).min(count - 1)))
                }
                Key::Named(NamedKey::ArrowLeft) if grid => {
                    Some(current.map_or(count - 1, |index| index.saturating_sub(1)))
                }
                Key::Named(NamedKey::Home) => Some(0),
                Key::Named(NamedKey::End) => Some(count - 1),
                Key::Named(NamedKey::PageDown) => {
                    Some(current.unwrap_or(0).saturating_add(page).min(count - 1))
                }
                Key::Named(NamedKey::PageUp) => Some(current.unwrap_or(0).saturating_sub(page)),
                _ => None,
            }
        });
        if let Some(index) = target {
            if event.modifiers.primary() && !event.modifiers.shift {
                self.update_selection(|state| {
                    let tab = state.tab_mut();
                    tab.navigation.view_mut().cursor = tab.rows.get(index).map(Row::key);
                });
            } else {
                self.select_row(index, event.modifiers);
            }
            self.reveal_cursor(index);
            event.prevent_default();
            event.stop_propagation();
        }
    }
    fn reveal_cursor(&self, index: usize) {
        let grid = self.read(|state| state.tab().mode == ViewMode::Grid);
        scroll_row_into_view(
            self.0.list,
            if grid {
                index / self.grid_columns()
            } else {
                index
            },
            self.row_height(),
        );
    }
    pub(super) fn search_key(&self, event: &KeyboardEvent) {
        if self.0.filter_node.is_composing() {
            return;
        }
        let target = self.read(|state| match event.key {
            Key::Named(NamedKey::ArrowDown) => (!state.tab().rows.is_empty()).then_some(0),
            Key::Named(NamedKey::ArrowUp) => state.tab().rows.len().checked_sub(1),
            _ => None,
        });
        if let Some(index) = target {
            self.select_row(index, Modifiers::default());
            self.focus_content();
            self.reveal_cursor(index);
            event.prevent_default();
            event.stop_propagation();
        }
    }
    pub(super) fn key_down(&self, event: &KeyboardEvent) {
        if self.menu_key(event) {
            return;
        }
        if !matches!(self.0.prompt.get_untracked(), Prompt::Closed)
            || self.0.connector.is_connecting()
        {
            return;
        }
        if self.pane() != Pane::Files
            && (event.key == Key::Named(NamedKey::Escape)
                || (event.modifiers.primary() && event.key == Key::Character("w".into()))
                || (event.modifiers.alt && event.key == Key::Named(NamedKey::ArrowLeft)))
        {
            self.back_page();
            event.prevent_default();
            event.stop_propagation();
            return;
        }
        if self.picker_key(event) {
            event.prevent_default();
            event.stop_propagation();
            return;
        }
        if event.modifiers.primary()
            && event.modifiers.shift
            && !event.modifiers.alt
            && matches!(&event.key,Key::Character(key) if key.eq_ignore_ascii_case("l"))
        {
            if gds::app::Gds::current().ui().logs().is_enabled() {
                gds::ui::logs::open();
                event.prevent_default();
                event.stop_propagation();
            }
            return;
        }
        if event.modifiers.primary()
            && !event.modifiers.shift
            && !event.modifiers.alt
            && let Key::Character(key) = &event.key
            && let Some(index) = key
                .parse::<usize>()
                .ok()
                .filter(|index| (1..=9).contains(index))
        {
            self.select_tab(index - 1);
            event.prevent_default();
            event.stop_propagation();
            return;
        }
        if let Some(command) =
            Command::from_key(event).filter(|command| !command.needs_content_focus())
        {
            self.dispatch(command);
            event.prevent_default();
            event.stop_propagation();
        }
    }
}
