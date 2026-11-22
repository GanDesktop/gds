//! Files command routing follows focus boundaries; native editors retain editing keys.

use std::{path::Path, sync::Arc};

use gds::ui::{Shortcut, logs, notifications};
use zgui::prelude::*;
use zgui::view::{EventCx, events};
use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey, Timestamp};

use super::{
    model::{AddressPresentation, Contents, Pane, Prompt, Row, Tab},
    ui::Browser,
};
use crate::{
    entities::{
        file::ViewMode,
        location::{Collection, Location, LocationTarget, volumes::DeviceAction},
    },
    shared::{
        lib::clipboard::Clipboard,
        ui::actions::{Command, ordinal_shortcut},
    },
};

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
    Volume(crate::entities::location::volumes::VolumeId),
}

impl Browser {
    pub(super) fn dispatch(&self, command: Command) {
        self.reset_typeahead();
        match command {
            Command::Menu => self.show_app_menu(),
            Command::Settings => self.toggle_settings(),
            Command::Help => self.show_help(),
            Command::NewFolder => self.new_folder(),
            Command::NewTab => self.new_tab(None),
            Command::NewWindow => {
                if self.read_untracked(|state| state.picker.is_some()) {
                    return;
                }
                let target = self.read_untracked(|state| {
                    state
                        .tab()
                        .navigation
                        .path()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| state.home.clone())
                });
                (self.0.new_window)(target);
            }
            Command::CloseTab => {
                let index = self.read_untracked(|state| state.active);
                self.close_tab(index);
            }
            Command::NextTab => {
                let target = self.read_untracked(|state| {
                    (state.tabs.len() > 1).then_some((state.active + 1) % state.tabs.len())
                });
                if let Some(index) = target {
                    self.select_tab(index);
                }
            }
            Command::PreviousTab => {
                let target = self.read_untracked(|state| {
                    let len = state.tabs.len();
                    (len > 1).then_some((state.active + len - 1) % len)
                });
                if let Some(index) = target {
                    self.select_tab(index);
                }
            }
            Command::Sidebar => self.toggle_sidebar(),
            Command::Terminal => self.toggle_terminal(),
            Command::EditLocation => self.start_address_edit(),
            Command::Back => self.history(false),
            Command::Forward => self.history(true),
            Command::Parent => self.parent(),
            Command::Refresh => self.refresh(),
            Command::Filter => {
                if self.filter_available_untracked() {
                    let current = self.0.filter.get_untracked();
                    self.0.filter_node.focus();
                    self.0.filter_node.set_value(&current);
                    self.0.filter_node.select_all();
                }
            }
            Command::Hidden => {
                self.edit_preferences(|prefs| {
                    prefs.files.show_hidden = !prefs.files.show_hidden;
                });
            }
            Command::Favorite => self.favorite(),
            Command::Open => self.open_selected(),
            Command::OpenInNewTab => self.open_selected_tab(),
            Command::Copy => self.copy(false),
            Command::Cut => self.copy(true),
            Command::Paste => self.paste(),
            Command::Undo => self.replay(false),
            Command::Redo => self.replay(true),
            Command::CopyPaths => self.copy_selected_paths(),
            Command::SelectAll => self.select_all(),
            Command::DeselectAll => {
                self.update_selection(|state| {
                    let view = state.tab_mut().navigation.view_mut();
                    view.selection.clear();
                });
            }
            Command::ContextMenu => self.show_context_menu(),
            Command::Rename => self.rename(),
            Command::Details => self.toggle_details(),
            Command::Columns => self.configure_columns(),
            Command::ViewMode => self.toggle_view(),
            Command::Sort => self.show_sort_menu(),
            Command::Trash => self.delete(false),
            Command::DeletePermanent => self.delete(true),
            Command::RestoreSelection => self.restore_trash(false, false),
            Command::PurgeSelection => self.restore_trash(true, false),
            Command::EmptyTrash => self.restore_trash(true, true),
            Command::ClearRecent => {
                let recent_tabs: Vec<u64> = self.update(|state| {
                    state.places.clear_recent();
                    state
                        .tabs
                        .iter()
                        .filter(|tab| matches!(tab.navigation.location(), Location::Recent))
                        .map(|tab| tab.id)
                        .collect()
                });
                self.save_places();
                for id in recent_tabs {
                    self.refresh_tab(id);
                }
            }
            Command::MountVolume => {
                let volume = self
                    .read_untracked(|state| {
                        state
                            .tab()
                            .selected()
                            .into_iter()
                            .find_map(|row| match row {
                                Row::Volume(volume) => Some(volume),
                                _ => None,
                            })
                    })
                    .or_else(|| {
                        self.read_untracked(|state| match state.tab().navigation.location() {
                            Location::Volume { identity, .. } => {
                                self.volume_at_location_untracked(identity)
                            }
                            _ => None,
                        })
                    });
                if let Some(volume) = volume
                    && volume.mount_points.is_empty()
                    && volume.mountable
                    && !volume.locked
                {
                    self.change_volume(DeviceAction::Mount(volume.id));
                }
            }
            Command::UnmountVolume => {
                let volume = self
                    .read_untracked(|state| {
                        state
                            .tab()
                            .selected()
                            .into_iter()
                            .find_map(|row| match row {
                                Row::Volume(volume) => Some(volume),
                                _ => None,
                            })
                    })
                    .or_else(|| {
                        self.read_untracked(|state| match state.tab().navigation.location() {
                            Location::Volume { identity, .. } => {
                                self.volume_at_location_untracked(identity)
                            }
                            _ => None,
                        })
                    });
                if let Some(volume) = volume
                    && volume.can_unmount()
                {
                    self.change_volume(DeviceAction::Unmount(volume.id));
                }
            }
            Command::FocusLocations => self.focus_sidebar_region(SidebarRegion::Locations),
            Command::FocusVolumes => self.focus_sidebar_region(SidebarRegion::Volumes),
            Command::FocusTabs => self.focus_sidebar_region(SidebarRegion::Tabs),
            Command::FocusContent => {
                self.focus_content();
                self.0.list_focus_visible.set(true);
            }
            Command::FocusNotifications => notifications::focus(),
            Command::CloseNotification => notifications::close_current(),
            Command::CloseAllNotifications => notifications::close_all(),
            Command::Cancel => self.cancel_escape(),
        }
    }

    fn cancel_active_drag(&self) -> bool {
        let had_sources = self.has_drag_sources_untracked();
        if had_sources {
            self.clear_drag_sources();
        }
        let had_term_drag = self.cancel_terminal_drag_hover();
        had_sources || had_term_drag
    }

    /// Exact 8-step `Escape` (`Command::Cancel`) priority chain matching `main`.
    /// NEVER cancels running background file transfers (`state.operation` / `background_tasks`)
    /// and NEVER clears unfocused filter text or file selection.
    fn cancel_escape(&self) {
        // 1. Open menu
        if self.0.menus.is_open() {
            self.0.menus.close();
            self.focus_content();
            return;
        }

        // 2. Open modal prompt or inspection dialog
        if !matches!(self.0.prompt.get_untracked(), Prompt::Closed) {
            self.cancel_prompt();
            return;
        }
        if self.has_open_inspection_dialog() {
            let _ = self.cancel_inspection_dialog();
            return;
        }

        // 3. Connecting remote server
        if self.0.connector.is_connecting_untracked() {
            self.0.connector.cancel();
            return;
        }

        // 4. Loading location
        if self.read_untracked(|state| state.tab().navigation.is_loading()) {
            self.cancel_location_load();
            return;
        }

        // 5. Active pointer drag (cancel without dropping, keep selection intact)
        if self.cancel_active_drag() {
            return;
        }

        // 6. Focused sidebar region
        if self.focused_sidebar_region().is_some() {
            if self.pane_untracked() == Pane::Files {
                let _ = self.cancel_clipboard_cut();
            }
            self.focus_content();
            return;
        }

        // 7 & 8. Transient local modes vs Cut ghosting vs Picker cancel
        let focused_node = zgui::view::current_host()
            .and_then(|host| host.focused().try_get_untracked().flatten());
        let filter_focused = focused_node.is_some_and(|node| self.0.filter_node.contains(node));
        let editing_address = !matches!(
            self.0.address_presentation.get_untracked(),
            AddressPresentation::Breadcrumbs
        );
        let local_mode = self.pane_untracked() != Pane::Files
            || editing_address
            || notifications::is_focused()
            || filter_focused;

        if !local_mode && self.cancel_clipboard_cut() {
            self.focus_content();
            return;
        }

        self.finish_address_edit();
        if self.pane_untracked() != Pane::Files {
            self.show_files();
        }
        Clipboard::current().refresh();
        self.focus_content();

        if !local_mode && self.read_untracked(|state| state.picker.is_some()) {
            self.cancel_picker();
        }
    }

    pub(super) fn sidebar_node(&self, region: SidebarRegion) -> NodeRef {
        match region {
            SidebarRegion::Tabs => self.0.tabs_node,
            SidebarRegion::Locations => self.0.locations_node,
            SidebarRegion::Volumes => self.0.volumes_node,
        }
    }

    pub(super) fn sidebar_focus_changed(&self, region: SidebarRegion, focused: bool) {
        if focused {
            self.0.sidebar_region.set(Some(region));
        } else if self.0.sidebar_region.get_untracked() == Some(region) {
            self.0.sidebar_region.set(None);
        }
    }

    pub(super) fn key_down(&self, event: &mut EventCx<'_, zgui::view::events::KeyDown>) {
        let modifiers = event.modifiers;
        if !event.repeat && is_logs_shortcut(event, modifiers) && logs::is_enabled() {
            logs::open();
            event.prevent_default();
            event.stop_propagation();
            return;
        }

        if self.0.menus.is_open() {
            if self.menu_key(event) {
                self.focus_content();
                event.prevent_default();
                event.stop_propagation();
                return;
            }
            if event.key == Key::Named(NamedKey::Escape) {
                self.0.menus.close();
                self.focus_content();
                event.prevent_default();
                event.stop_propagation();
                return;
            }
            return;
        }

        if self.has_open_inspection_dialog() {
            if event.key == Key::Named(NamedKey::Escape) && modifiers.is_empty() {
                let _ = self.cancel_inspection_dialog();
                event.prevent_default();
                event.stop_propagation();
            }
            return;
        }

        match self.0.prompt.get_untracked() {
            Prompt::Closed => {}
            Prompt::Name { .. } => {
                let _ = self.0.prompt_decisions.handle_key_down(event);
                // `NameForm` owns Enter/Escape and guards against IME preedit.
                return;
            }
            Prompt::About => {
                if self.0.prompt_decisions.handle_key_down(event) {
                    return;
                }
                if event.key == Key::Named(NamedKey::Escape) {
                    self.cancel_prompt();
                    event.prevent_default();
                    event.stop_propagation();
                }
                return;
            }
            Prompt::Confirm { .. } => {
                if self.0.prompt_decisions.handle_key_down(event) {
                    return;
                }
                if event.key == Key::Named(NamedKey::Escape) {
                    self.cancel_prompt();
                    event.prevent_default();
                    event.stop_propagation();
                } else if !event.repeat
                    && modifiers.is_empty()
                    && event.key == Key::Named(NamedKey::Enter)
                    && self.0.prompt_decisions.confirm()
                {
                    event.prevent_default();
                    event.stop_propagation();
                }
                return;
            }
        }

        if self.0.connector.is_connecting_untracked() {
            if event.key == Key::Named(NamedKey::Escape) && modifiers.is_empty() {
                self.0.connector.cancel();
                event.prevent_default();
                event.stop_propagation();
            }
            return;
        }

        if gds::ui::keytips::has_active_modal_scope() {
            return;
        }

        let pane = self.pane_untracked();
        if pane != Pane::Files {
            if !event.repeat
                && let Some(command) = resolve_shortcut(event, modifiers)
            {
                match command {
                    Command::Cancel | Command::Back | Command::CloseTab => {
                        self.back_page();
                        event.prevent_default();
                        event.stop_propagation();
                    }
                    Command::NextTab if pane == Pane::Settings => {
                        self.0.settings.cycle_page(false);
                        event.prevent_default();
                        event.stop_propagation();
                    }
                    Command::PreviousTab if pane == Pane::Settings => {
                        self.0.settings.cycle_page(true);
                        event.prevent_default();
                        event.stop_propagation();
                    }
                    Command::Settings
                    | Command::Help
                    | Command::Sidebar
                    | Command::Terminal
                    | Command::FocusNotifications
                    | Command::CloseNotification
                    | Command::CloseAllNotifications => {
                        self.dispatch(command);
                        event.prevent_default();
                        event.stop_propagation();
                    }
                    _ => {}
                }
            }
            return;
        }

        if self.terminal_focused() {
            if !event.repeat
                && let Some(command) = resolve_shortcut(event, modifiers)
                && matches!(
                    command,
                    Command::Settings
                        | Command::Help
                        | Command::Terminal
                        | Command::FocusNotifications
                        | Command::CloseNotification
                        | Command::CloseAllNotifications
                )
            {
                self.dispatch(command);
                event.prevent_default();
                event.stop_propagation();
            }
            return;
        }

        let focused_node = zgui::view::current_host()
            .and_then(|host| host.focused().try_get_untracked().flatten());
        if focused_node.is_some_and(|node| self.0.address_node.contains(node)) {
            return;
        }

        // Picker Alt-mnemonics and Ctrl+Enter apply across the picker workspace (including Save/Filter inputs).
        if !event.repeat && self.picker_key(event, modifiers) {
            event.prevent_default();
            event.stop_propagation();
            return;
        }

        if focused_node.is_some_and(|node| self.0.filter_node.contains(node)) {
            if self.handle_search_key(event, modifiers) {
                event.prevent_default();
                event.stop_propagation();
            }
            return;
        }

        if let Some(region) = self.focused_sidebar_region()
            && self.handle_sidebar_key(event, modifiers, region)
        {
            event.prevent_default();
            event.stop_propagation();
            return;
        }

        if !event.repeat
            && let Some(command) = resolve_shortcut(event, modifiers)
            && (!command.needs_content_focus() || self.0.list_focused.get())
        {
            self.dispatch(command);
            event.prevent_default();
            event.stop_propagation();
            return;
        }

        if !event.repeat
            && let Some(index) = matching_ordinal_tab(event, modifiers)
        {
            let tab_count = self.read_untracked(|state| state.tabs.len());
            if index < tab_count {
                self.select_tab(index);
                event.prevent_default();
                event.stop_propagation();
                return;
            }
        }

        if self.0.list_focused.get() && self.handle_content_key(event, modifiers) {
            event.prevent_default();
            event.stop_propagation();
        }
    }

    pub(super) fn select_row(&self, index: usize, modifiers: Modifiers) {
        self.reset_typeahead();
        self.select_row_inner(index, modifiers);
    }

    fn select_row_from_typeahead(&self, index: usize) {
        self.select_row_inner(index, Modifiers::NONE);
        self.0.list_focus_visible.set(true);
        self.scroll_row_into_view(index);
    }

    fn select_row_inner(&self, index: usize, modifiers: Modifiers) {
        let changed = self.update(|state| {
            let allows_multiple = state.allows_multiple();
            let tab = state.tab_mut();
            let Some(target_key) = tab.rows.get(index).map(Row::key) else {
                return false;
            };
            let backing = Arc::clone(&tab.backing);
            let view = tab.navigation.view_mut();
            if modifiers.shift() && allows_multiple {
                let anchor_index = view
                    .anchor
                    .as_ref()
                    .and_then(|anchor| backing.position(anchor))
                    .unwrap_or(index);
                view.selection.set_range(
                    &backing,
                    anchor_index.min(index),
                    anchor_index.max(index),
                );
                if view.anchor.is_none() {
                    view.anchor = Some(target_key.clone());
                }
                view.cursor = Some(target_key);
            } else if modifiers.primary() && allows_multiple {
                view.selection.toggle(target_key.clone());
                view.anchor = Some(target_key.clone());
                view.cursor = Some(target_key);
            } else {
                view.selection.set_single(target_key.clone());
                view.anchor = Some(target_key.clone());
                view.cursor = Some(target_key);
            }
            true
        });
        if changed {
            self.0.list_focus_visible.set(false);
            self.update_selection(|_| {});
        }
    }

    pub(super) fn select_all(&self) {
        let changed = self.update(|state| {
            if !state.allows_multiple() {
                return false;
            }
            let tab = state.tab_mut();
            if tab.rows.is_empty() {
                return false;
            }
            let first = tab.rows.first().map(Row::key);
            let last = tab.rows.last().map(Row::key);
            let keys: Vec<_> = tab.rows.iter().map(Row::key).collect();
            let view = tab.navigation.view_mut();
            view.selection.replace(keys);
            if view.anchor.is_none() {
                view.anchor = first;
            }
            view.cursor = last;
            true
        });
        if changed {
            self.update_selection(|_| {});
        }
    }

    pub(super) fn focused_sidebar_region(&self) -> Option<SidebarRegion> {
        let focused = zgui::view::current_host()
            .and_then(|host| host.focused().try_get_untracked().flatten());
        if focused.is_some_and(|node| self.0.tabs_node.contains(node)) {
            Some(SidebarRegion::Tabs)
        } else if focused.is_some_and(|node| self.0.locations_node.contains(node)) {
            Some(SidebarRegion::Locations)
        } else if focused.is_some_and(|node| self.0.volumes_node.contains(node)) {
            Some(SidebarRegion::Volumes)
        } else {
            self.0
                .sidebar_region
                .get_untracked()
                .filter(|_| !self.0.list_focused.get())
        }
    }

    fn focus_sidebar_region(&self, region: SidebarRegion) {
        if !self.0.sidebar.get_untracked() {
            self.0.sidebar.set(true);
        }
        if region == SidebarRegion::Volumes {
            self.0.sidebar_reveal_volumes.set(true);
        }
        let rows = self.sidebar_rows(region);
        let current = self.0.sidebar_cursor.get_untracked();
        let valid = current.as_ref().is_some_and(|item| rows.contains(item));
        if !valid {
            self.0.sidebar_cursor.set(rows.into_iter().next());
        }
        self.0.sidebar_region.set(Some(region));
        self.sidebar_node(region).focus();
        self.changed();
    }

    fn sidebar_rows(&self, region: SidebarRegion) -> Vec<SidebarRow> {
        match region {
            SidebarRegion::Tabs => self.read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .map(|tab| SidebarRow::Tab(tab.id))
                    .collect()
            }),
            SidebarRegion::Locations => {
                let tags_enabled = self.tags_enabled_untracked();
                LocationTarget::ALL
                    .into_iter()
                    .filter(|target| {
                        *target != LocationTarget::Collection(Collection::Tags) || tags_enabled
                    })
                    .map(SidebarRow::Location)
                    .collect()
            }
            SidebarRegion::Volumes => self
                .0
                .volumes
                .snapshot_untracked()
                .volumes
                .into_iter()
                .filter(|volume| volume.visible)
                .map(|volume| SidebarRow::Volume(volume.id))
                .collect(),
        }
    }

    fn handle_sidebar_key(
        &self,
        event: &KeyEvent,
        modifiers: Modifiers,
        region: SidebarRegion,
    ) -> bool {
        if modifiers.control() || modifiers.alt() || modifiers.meta() {
            return false;
        }
        let rows = self.sidebar_rows(region);
        if rows.is_empty() {
            return false;
        }
        let current_idx = self
            .0
            .sidebar_cursor
            .get_untracked()
            .and_then(|cursor| rows.iter().position(|row| row == &cursor));

        match event.key {
            Key::Named(NamedKey::ArrowDown) => {
                let next = match current_idx {
                    Some(idx) => (idx + 1).min(rows.len() - 1),
                    None => 0,
                };
                self.0.sidebar_cursor.set(Some(rows[next].clone()));
                true
            }
            Key::Named(NamedKey::ArrowUp) => {
                let next = match current_idx {
                    Some(idx) => idx.saturating_sub(1),
                    None => rows.len() - 1,
                };
                self.0.sidebar_cursor.set(Some(rows[next].clone()));
                true
            }
            Key::Named(NamedKey::Home) => {
                self.0.sidebar_cursor.set(rows.into_iter().next());
                true
            }
            Key::Named(NamedKey::End) => {
                self.0.sidebar_cursor.set(rows.into_iter().last());
                true
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                let chosen = current_idx
                    .and_then(|idx| rows.get(idx))
                    .or_else(|| rows.first())
                    .cloned();
                let Some(row) = chosen else {
                    return false;
                };
                self.activate_sidebar_row(row);
                true
            }
            Key::Named(NamedKey::Escape) => {
                if self.pane_untracked() == Pane::Files {
                    let _ = self.cancel_clipboard_cut();
                }
                self.focus_content();
                true
            }
            _ if event.key == Key::character(" ") => {
                let chosen = current_idx
                    .and_then(|idx| rows.get(idx))
                    .or_else(|| rows.first())
                    .cloned();
                let Some(row) = chosen else {
                    return false;
                };
                self.activate_sidebar_row(row);
                true
            }
            _ => false,
        }
    }

    fn activate_sidebar_row(&self, row: SidebarRow) {
        match row {
            SidebarRow::Tab(id) => {
                if let Some(index) = self.tab_index(id) {
                    self.select_tab(index);
                    self.focus_content();
                }
            }
            SidebarRow::Location(target) => {
                let home = self.read_untracked(|state| state.home.clone());
                self.open(target.location(&home));
                self.focus_content();
            }
            SidebarRow::Volume(id) => {
                self.open_volume(&id, false);
                self.focus_content();
            }
        }
    }

    fn handle_search_key(&self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        if !modifiers.is_empty() {
            if !event.repeat
                && let Some(command) = resolve_shortcut(event, modifiers)
                && matches!(
                    command,
                    Command::Settings
                        | Command::Help
                        | Command::Sidebar
                        | Command::Terminal
                        | Command::Filter
                        | Command::FocusLocations
                        | Command::FocusVolumes
                        | Command::FocusTabs
                        | Command::FocusContent
                        | Command::FocusNotifications
                        | Command::CloseNotification
                        | Command::CloseAllNotifications
                )
            {
                self.dispatch(command);
                return true;
            }
            return false;
        }

        match event.key {
            Key::Named(NamedKey::Escape) => {
                if !self.0.filter.get_untracked().is_empty() {
                    self.filter(String::new());
                    self.0.filter_node.set_value("");
                } else {
                    self.focus_content();
                }
                true
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.focus_content();
                let row_count = self.read_untracked(|state| state.tab().rows.len());
                if row_count > 0 {
                    self.select_row(0, Modifiers::NONE);
                    self.0.list_focus_visible.set(true);
                    self.scroll_row_into_view(0);
                }
                true
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.focus_content();
                let row_count = self.read_untracked(|state| state.tab().rows.len());
                if let Some(last) = row_count.checked_sub(1) {
                    self.select_row(last, Modifiers::NONE);
                    self.0.list_focus_visible.set(true);
                    self.scroll_row_into_view(last);
                }
                true
            }
            Key::Named(NamedKey::Enter) => {
                self.focus_content();
                let should_select_first = self.read_untracked(|state| {
                    let tab = state.tab();
                    !tab.rows.is_empty() && tab.navigation.view().selection.is_empty()
                });
                if should_select_first {
                    self.select_row(0, Modifiers::NONE);
                    self.0.list_focus_visible.set(true);
                    self.scroll_row_into_view(0);
                }
                true
            }
            _ => false,
        }
    }

    fn content_surface_focused(&self) -> bool {
        if zgui::view::focus_is_text_entry() {
            return false;
        }
        let Some(active) = zgui::view::current_host()
            .and_then(|host| host.focused().try_get_untracked().flatten())
        else {
            return false;
        };
        let content_id = self.0.content_focus.get_untracked();
        let list_id = self.0.list.get_untracked();
        content_id == Some(active) || list_id == Some(active) || self.0.list.contains(active)
    }

    fn can_handle_content_text_input(&self) -> bool {
        self.pane_untracked() == Pane::Files
            && !self.0.menus.is_open()
            && !self.has_open_inspection_dialog()
            && matches!(self.0.prompt.get_untracked(), Prompt::Closed)
            && !self.0.connector.is_connecting_untracked()
            && !gds::ui::keytips::has_active_modal_scope()
            && !self.terminal_focused()
            && self.focused_sidebar_region().is_none()
            && self.content_surface_focused()
    }

    pub(super) fn handle_content_text_input(&self, inserted: &str, now: Timestamp) -> bool {
        if inserted.is_empty()
            || inserted.chars().all(char::is_control)
            || !self.can_handle_content_text_input()
        {
            return false;
        }
        if self.0.preferences.snapshot().files.auto_focus_search {
            self.reset_typeahead();
            if inserted.chars().all(char::is_whitespace) || !self.filter_available_untracked() {
                return false;
            }
            let mut next = self.0.filter.get_untracked();
            next.push_str(inserted);
            self.filter(next.clone());
            self.0.filter_node.focus();
            self.0.filter_node.set_value(&next);
            self.0.filter_node.set_selection(next.len()..next.len());
            return true;
        }
        self.typeahead_input(inserted, now)
    }

    fn typeahead_input(&self, inserted: &str, now: Timestamp) -> bool {
        let matched = {
            let state = self.0.state.borrow();
            let tab = state.tab();
            if matches!(tab.contents, Contents::Network { .. })
                || tab.navigation.is_loading()
                || tab.rows.is_empty()
            {
                return false;
            }
            let current = current_row_index(tab);
            let mut ta = self.0.typeahead.borrow_mut();
            let before_active = ta.is_active(now);
            let hit = ta.push_and_match(inserted, now, tab.rows.len(), current, |idx, prefix| {
                tab.rows[idx].matches_prefix(prefix)
            });
            let after_active = ta.is_active(now);
            if hit.is_none() && !before_active && !after_active {
                return false;
            }
            hit
        };

        if let Some(target) = matched {
            self.select_row_from_typeahead(target);
        }
        true
    }

    fn typeahead_backspace(&self, now: Timestamp) -> bool {
        let outcome = {
            let state = self.0.state.borrow();
            let tab = state.tab();
            if matches!(tab.contents, Contents::Network { .. })
                || tab.navigation.is_loading()
                || tab.rows.is_empty()
            {
                self.0.typeahead.borrow_mut().reset();
                return false;
            }
            let current = current_row_index(tab);
            self.0.typeahead.borrow_mut().pop_and_match(
                now,
                tab.rows.len(),
                current,
                |idx, prefix| tab.rows[idx].matches_prefix(prefix),
            )
        };

        let Some(matched) = outcome else {
            return false;
        };
        if let Some(target) = matched {
            self.select_row_from_typeahead(target);
        }
        true
    }

    fn handle_content_key(
        &self,
        event: &EventCx<'_, events::KeyDown>,
        modifiers: Modifiers,
    ) -> bool {
        let (count, mode, current_idx) = self.read_untracked(|state| {
            let tab = state.tab();
            (tab.rows.len(), tab.mode, current_row_index(tab))
        });

        if event.key == Key::Named(NamedKey::Enter) {
            self.reset_typeahead();
            if modifiers.shift() || modifiers.control() {
                self.open_selected_tab();
            } else {
                self.open_selected();
            }
            return true;
        }

        let no_command_mods = !modifiers.control() && !modifiers.alt() && !modifiers.meta();

        if event.key == Key::Named(NamedKey::Backspace)
            && no_command_mods
            && !self.0.preferences.snapshot().files.auto_focus_search
            && self.content_surface_focused()
            && self.typeahead_backspace(event.timestamp)
        {
            return true;
        }

        if event.key == Key::Named(NamedKey::Space) || event.key == Key::character(" ") {
            if no_command_mods
                && !self.0.preferences.snapshot().files.auto_focus_search
                && self.content_surface_focused()
                && self.0.typeahead.borrow().is_active(event.timestamp)
            {
                return self.typeahead_input(" ", event.timestamp);
            }
            if count == 0 {
                return false;
            }
            let idx = current_idx.unwrap_or(0);
            if modifiers.primary() {
                self.select_row(idx, Modifiers::PRIMARY);
            } else {
                self.select_row(idx, Modifiers::NONE);
            }
            self.0.list_focus_visible.set(true);
            return true;
        }

        if no_command_mods && let Key::Character(text) = &event.key {
            return self.handle_content_text_input(text.as_str(), event.timestamp);
        }

        if count == 0 {
            return false;
        }

        let columns = match mode {
            ViewMode::List => 1,
            ViewMode::Grid => self.grid_columns_untracked().max(1),
        };
        let page_step = self.page_step_rows(mode, columns);
        let cur = current_idx;
        let next = match event.key {
            Key::Named(NamedKey::ArrowDown) => Some(match cur {
                Some(idx) => (idx + columns).min(count - 1),
                None => 0,
            }),
            Key::Named(NamedKey::ArrowUp) => Some(match cur {
                Some(idx) => idx.saturating_sub(columns),
                None => count - 1,
            }),
            Key::Named(NamedKey::ArrowRight) if mode == ViewMode::Grid => Some(match cur {
                Some(idx) => (idx + 1).min(count - 1),
                None => 0,
            }),
            Key::Named(NamedKey::ArrowLeft) if mode == ViewMode::Grid => Some(match cur {
                Some(idx) => idx.saturating_sub(1),
                None => count - 1,
            }),
            Key::Named(NamedKey::Home) => Some(0),
            Key::Named(NamedKey::End) => Some(count - 1),
            Key::Named(NamedKey::PageDown) => Some(match cur {
                Some(idx) => (idx + page_step).min(count - 1),
                None => 0,
            }),
            Key::Named(NamedKey::PageUp) => Some(match cur {
                Some(idx) => idx.saturating_sub(page_step),
                None => count - 1,
            }),
            _ => None,
        };
        let Some(target) = next else {
            return false;
        };
        if modifiers.primary() && !modifiers.shift() {
            self.reset_typeahead();
            let moved = self.update(|state| {
                let tab = state.tab_mut();
                let Some(key) = tab.rows.get(target).map(Row::key) else {
                    return false;
                };
                tab.navigation.view_mut().cursor = Some(key);
                true
            });
            if moved {
                self.0.list_focus_visible.set(true);
                self.update_selection(|_| {});
                self.scroll_row_into_view(target);
            }
        } else {
            let sel_mod = if modifiers.shift() {
                Modifiers::SHIFT
            } else {
                Modifiers::NONE
            };
            self.select_row(target, sel_mod);
            self.0.list_focus_visible.set(true);
            self.scroll_row_into_view(target);
        }
        true
    }

    fn page_step_rows(&self, mode: ViewMode, columns: usize) -> usize {
        let scale = self.0.list.scale().max(1.0);
        let viewport_h = (self.0.list.scroll_position().scrollport.height.0 / scale).max(300.0);
        let row_h = self.row_height_untracked().max(24.0);
        let visible_rows = (viewport_h / row_h).floor().max(1.0) as usize;
        match mode {
            ViewMode::List => visible_rows,
            ViewMode::Grid => visible_rows * columns,
        }
    }
}

fn current_row_index(tab: &Tab) -> Option<usize> {
    tab.navigation
        .view()
        .cursor
        .as_ref()
        .and_then(|key| tab.backing.position(key))
        .or_else(|| {
            tab.navigation
                .view()
                .selection
                .as_slice()
                .first()
                .and_then(|key| tab.backing.position(key))
        })
}

fn resolve_shortcut(event: &KeyEvent, modifiers: Modifiers) -> Option<Command> {
    if let Some(command) = Command::from_key_event(event, modifiers) {
        return Some(command);
    }
    let ctrl_y = Shortcut::new(Key::character("y"), Modifiers::CONTROL);
    if ctrl_y.matches(&event.key, modifiers)
        || ctrl_y.matches(&event.key_without_modifiers, modifiers)
    {
        return Some(Command::Redo);
    }
    let ctrl_t = Shortcut::new(Key::character("t"), Modifiers::CONTROL);
    if ctrl_t.matches(&event.key, modifiers)
        || ctrl_t.matches(&event.key_without_modifiers, modifiers)
    {
        return Some(Command::NewTab);
    }
    None
}

fn is_logs_shortcut(event: &KeyEvent, modifiers: Modifiers) -> bool {
    let shortcut = Shortcut::new(Key::character("l"), Modifiers::CONTROL | Modifiers::SHIFT);
    shortcut.matches(&event.key, modifiers)
        || shortcut.matches(&event.key_without_modifiers, modifiers)
}

fn matching_ordinal_tab(event: &KeyEvent, modifiers: Modifiers) -> Option<usize> {
    (0..9).find(|&pos| {
        ordinal_shortcut(pos).is_some_and(|shortcut| {
            shortcut.matches(&event.key, modifiers)
                || shortcut.matches(&event.key_without_modifiers, modifiers)
        })
    })
}
