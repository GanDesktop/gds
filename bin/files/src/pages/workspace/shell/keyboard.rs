//! Files command routing follows focus boundaries; native editors retain editing keys.

use std::{path::Path, sync::Arc};

use gds::ui::{Shortcut, logs, notifications};
use zgui::prelude::*;
use zgui::view::{EventCx, events};
use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey, Timestamp};

use crate::{
    entities::{
        file::ViewMode,
        location::{Collection, Location, LocationTarget, volumes::DeviceAction},
    },
    pages::workspace::{
        navigation::{Navigator, QuickAccess},
        operations::{DeviceOps, Dispatch, Lifecycle, LocalOps, PickerOps},
        panes::{Inspection, MenuHost, PickerView, TerminalDock},
        projection::{Geometry, Rows, UiState},
        state::{
            context::Context,
            model::{AddressPresentation, Contents, Pane, Prompt, Row, Tab},
            ui_flags::{
                AccordionSelection, CollapsibleSection, FilterFocusMode, SidebarRegion, SidebarRow,
            },
        },
    },
    shared::{
        lib::clipboard::Clipboard,
        ui::actions::{Command, ordinal_shortcut},
    },
};

pub(in crate::pages::workspace) struct Keyboard<'a>(pub &'a Context);

impl Keyboard<'_> {
    pub(in crate::pages::workspace) fn dispatch(&self, command: Command) {
        let ctx = self.0;
        ctx.reset_typeahead();
        match command {
            Command::Menu => MenuHost(ctx).show_app_menu(),
            Command::Settings => UiState(ctx).toggle_settings(),
            Command::Help => MenuHost(ctx).show_help(),
            Command::NewFolder => LocalOps(ctx).new_folder(),
            Command::NewTab => Navigator(ctx).new_tab(None),
            Command::NewWindow => {
                if ctx.read_untracked(|state| state.picker.is_some()) {
                    return;
                }
                let target = ctx.read_untracked(|state| {
                    state
                        .tab()
                        .navigation
                        .path()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| state.home.clone())
                });
                (ctx.0.new_window)(target);
            }
            Command::CloseTab => {
                let index = ctx.read_untracked(|state| state.active);
                Navigator(ctx).close_tab(index);
            }
            Command::NextTab => {
                let target = ctx.read_untracked(|state| {
                    (state.tabs.len() > 1).then_some((state.active + 1) % state.tabs.len())
                });
                if let Some(index) = target {
                    Navigator(ctx).select_tab(index);
                }
            }
            Command::PreviousTab => {
                let target = ctx.read_untracked(|state| {
                    let len = state.tabs.len();
                    (len > 1).then_some((state.active + len - 1) % len)
                });
                if let Some(index) = target {
                    Navigator(ctx).select_tab(index);
                }
            }
            Command::Sidebar => UiState(ctx).toggle_sidebar(),
            Command::Terminal => TerminalDock(ctx).toggle_terminal(),
            Command::EditLocation => UiState(ctx).start_address_edit(),
            Command::Back => Navigator(ctx).history(false),
            Command::Forward => Navigator(ctx).history(true),
            Command::Parent => Navigator(ctx).parent(),
            Command::Refresh => Navigator(ctx).refresh(),
            Command::Filter => UiState(ctx).activate_filter(),
            Command::Hidden => {
                ctx.edit_preferences(|prefs| {
                    prefs.files.show_hidden = !prefs.files.show_hidden;
                });
            }
            Command::Favorite => Navigator(ctx).favorite(),
            Command::Open => Dispatch(ctx).open_selected(),
            Command::OpenInNewTab => MenuHost(ctx).open_selected_tab(),
            Command::Copy => LocalOps(ctx).copy(false),
            Command::Cut => LocalOps(ctx).copy(true),
            Command::Paste => Dispatch(ctx).paste(),
            Command::Undo => LocalOps(ctx).replay(false),
            Command::Redo => LocalOps(ctx).replay(true),
            Command::CopyPaths => MenuHost(ctx).copy_selected_paths(),
            Command::SelectAll => self.select_all(),
            Command::DeselectAll => {
                ctx.update_selection(|state| {
                    let view = state.tab_mut().navigation.view_mut();
                    view.selection.clear();
                });
            }
            Command::ContextMenu => MenuHost(ctx).show_context_menu(),
            Command::Rename => Dispatch(ctx).rename(),
            Command::Details => Inspection(ctx).toggle_details(),
            Command::Columns => Rows(ctx).configure_columns(),
            Command::ViewMode => Navigator(ctx).toggle_view(),
            Command::Sort => MenuHost(ctx).show_sort_menu(),
            Command::Trash => Dispatch(ctx).delete(false),
            Command::DeletePermanent => Dispatch(ctx).delete(true),
            Command::RestoreSelection => LocalOps(ctx).restore_trash(false, false),
            Command::PurgeSelection => LocalOps(ctx).restore_trash(true, false),
            Command::EmptyTrash => LocalOps(ctx).restore_trash(true, true),
            Command::ClearRecent => LocalOps(ctx).clear_recent(),
            Command::MountVolume => {
                let volume = ctx
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
                        ctx.read_untracked(|state| match state.tab().navigation.location() {
                            Location::Volume { identity, .. } => {
                                Rows(ctx).volume_at_location_untracked(identity)
                            }
                            _ => None,
                        })
                    });
                if let Some(volume) = volume
                    && volume.mount_points.is_empty()
                    && volume.mountable
                    && !volume.locked
                {
                    MenuHost(ctx).change_volume(DeviceAction::Mount(volume.id));
                }
            }
            Command::UnmountVolume => {
                let volume = ctx
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
                        ctx.read_untracked(|state| match state.tab().navigation.location() {
                            Location::Volume { identity, .. } => {
                                Rows(ctx).volume_at_location_untracked(identity)
                            }
                            _ => None,
                        })
                    });
                if let Some(volume) = volume
                    && volume.can_unmount()
                {
                    MenuHost(ctx).change_volume(DeviceAction::Unmount(volume.id));
                }
            }
            Command::FocusQuickAccess => self.focus_sidebar_region(SidebarRegion::QuickAccess),
            Command::FocusLocations => self.focus_sidebar_region(SidebarRegion::Locations),
            Command::FocusVolumes => self.focus_sidebar_region(SidebarRegion::Volumes),
            Command::FocusTabs => self.focus_sidebar_region(SidebarRegion::Tabs),
            Command::FocusContent => {
                ctx.focus_content();
                ctx.0.list_focus_visible.set(true);
            }
            Command::FocusNotifications => notifications::focus(),
            Command::CloseNotification => notifications::close_current(),
            Command::CloseAllNotifications => notifications::close_all(),
            Command::Cancel => self.cancel_escape(),
        }
    }

    fn cancel_active_drag(&self) -> bool {
        let had_sources = self.0.has_drag_sources_untracked();
        if had_sources {
            self.0.clear_drag_sources();
        }
        let had_term_drag = TerminalDock(self.0).cancel_terminal_drag_hover();
        had_sources || had_term_drag
    }

    /// Exact 8-step `Escape` (`Command::Cancel`) priority chain matching `main`.
    /// NEVER cancels running background file transfers (`state.operation` / `background_tasks`)
    /// and NEVER clears unfocused filter text or file selection.
    fn cancel_escape(&self) {
        let ctx = self.0;
        // 1. Open menu
        if ctx.0.menus.is_open() {
            ctx.0.menus.close();
            ctx.focus_content();
            return;
        }

        // 2. Open modal prompt or inspection dialog
        if !matches!(ctx.0.prompt.get_untracked(), Prompt::Closed) {
            ctx.cancel_prompt();
            return;
        }
        let inspection = Inspection(ctx);
        if inspection.has_open_inspection_dialog() {
            let _ = inspection.cancel_inspection_dialog();
            return;
        }

        // 3. Connecting remote server
        if ctx.0.connector.is_connecting_untracked() {
            ctx.0.connector.cancel();
            return;
        }

        // 4. Loading location
        if ctx.read_untracked(|state| state.tab().navigation.is_loading()) {
            Navigator(ctx).cancel_location_load();
            return;
        }

        // 5. Active pointer drag (cancel without dropping, keep selection intact)
        if self.cancel_active_drag() {
            return;
        }

        // 6. Focused sidebar region
        let ui_state = UiState(ctx);
        if ui_state.focused_sidebar_region().is_some() {
            if ctx.pane_untracked() == Pane::Files {
                let _ = Lifecycle(ctx).cancel_clipboard_cut();
            }
            ctx.focus_content();
            return;
        }

        // 7 & 8. Transient local modes vs Cut ghosting vs Picker cancel
        let focused_node = zgui::view::current_host()
            .and_then(|host| host.focused().try_get_untracked().flatten());
        let filter_focused = focused_node.is_some_and(|node| ctx.0.filter_node.contains(node));
        let editing_address = !matches!(
            ctx.0.address_presentation.get_untracked(),
            AddressPresentation::Breadcrumbs
        );
        let local_mode = ctx.pane_untracked() != Pane::Files
            || editing_address
            || notifications::is_focused()
            || filter_focused;

        if !local_mode && Lifecycle(ctx).cancel_clipboard_cut() {
            ctx.focus_content();
            return;
        }

        ui_state.finish_address_edit();
        if ctx.pane_untracked() != Pane::Files {
            ctx.show_files();
        }
        Clipboard::current().refresh();
        ctx.focus_content();

        if !local_mode && ctx.read_untracked(|state| state.picker.is_some()) {
            PickerOps(ctx).cancel_picker();
        }
    }

    pub(in crate::pages::workspace) fn sidebar_node(&self, region: SidebarRegion) -> NodeRef {
        match region {
            SidebarRegion::Tabs => self.0.0.tabs_node,
            SidebarRegion::QuickAccess => self.0.0.quick_access_node,
            SidebarRegion::Locations => self.0.0.locations_node,
            SidebarRegion::Volumes => self.0.0.volumes_node,
        }
    }

    pub(in crate::pages::workspace) fn key_down(
        &self,
        event: &mut EventCx<'_, zgui::view::events::KeyDown>,
    ) {
        let ctx = self.0;
        let modifiers = event.modifiers;
        if !event.repeat && is_logs_shortcut(event, modifiers) && logs::is_enabled() {
            logs::open();
            event.prevent_default();
            event.stop_propagation();
            return;
        }

        if ctx.0.menus.is_open() {
            if MenuHost(ctx).menu_key(event) {
                ctx.focus_content();
                event.prevent_default();
                event.stop_propagation();
                return;
            }
            if event.key == Key::Named(NamedKey::Escape) {
                ctx.0.menus.close();
                ctx.focus_content();
                event.prevent_default();
                event.stop_propagation();
                return;
            }
            return;
        }

        let inspection = Inspection(ctx);
        if inspection.has_open_inspection_dialog() {
            if event.key == Key::Named(NamedKey::Escape) && modifiers.is_empty() {
                let _ = inspection.cancel_inspection_dialog();
                event.prevent_default();
                event.stop_propagation();
            }
            return;
        }

        match ctx.0.prompt.get_untracked() {
            Prompt::Closed => {}
            Prompt::Name { .. } | Prompt::Conflict { .. } => {
                let _ = ctx.0.prompt_decisions.handle_key_down(event);
                // `NameForm` / `ConflictDialog` own Enter/Escape and guard against IME preedit.
                return;
            }
            Prompt::About => {
                if ctx.0.prompt_decisions.handle_key_down(event) {
                    return;
                }
                if event.key == Key::Named(NamedKey::Escape)
                    || gds::ui::feedback::dialog::DialogCommand::Cancel
                        .matches_event(event, modifiers)
                {
                    ctx.cancel_prompt();
                    event.prevent_default();
                    event.stop_propagation();
                }
                return;
            }
            Prompt::Confirm { .. } => {
                if ctx.0.prompt_decisions.handle_key_down(event) {
                    return;
                }
                if event.key == Key::Named(NamedKey::Escape) {
                    ctx.cancel_prompt();
                    event.prevent_default();
                    event.stop_propagation();
                } else if !event.repeat
                    && modifiers.is_empty()
                    && event.key == Key::Named(NamedKey::Enter)
                    && ctx.0.prompt_decisions.confirm()
                {
                    event.prevent_default();
                    event.stop_propagation();
                }
                return;
            }
        }

        if ctx.0.connector.is_connecting_untracked() {
            if event.key == Key::Named(NamedKey::Escape) && modifiers.is_empty() {
                ctx.0.connector.cancel();
                event.prevent_default();
                event.stop_propagation();
            }
            return;
        }

        if gds::ui::keytips::has_active_modal_scope() {
            return;
        }

        let pane = ctx.pane_untracked();
        if pane != Pane::Files {
            if !event.repeat
                && let Some(command) = resolve_shortcut(event, modifiers)
            {
                match command {
                    Command::Cancel | Command::Back | Command::CloseTab => {
                        ctx.back_page();
                        event.prevent_default();
                        event.stop_propagation();
                    }
                    Command::NextTab if pane == Pane::Settings => {
                        ctx.0.settings.cycle_page(false);
                        event.prevent_default();
                        event.stop_propagation();
                    }
                    Command::PreviousTab if pane == Pane::Settings => {
                        ctx.0.settings.cycle_page(true);
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

        if TerminalDock(ctx).terminal_focused() {
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
        if focused_node.is_some_and(|node| ctx.0.address_node.contains(node)) {
            return;
        }

        // Picker Alt-mnemonics and Ctrl+Enter apply across the picker workspace (including Save/Filter inputs).
        if !event.repeat && PickerView(ctx).picker_key(event, modifiers) {
            event.prevent_default();
            event.stop_propagation();
            return;
        }

        if focused_node.is_some_and(|node| ctx.0.filter_node.contains(node)) {
            if self.handle_search_key(event, modifiers) {
                event.prevent_default();
                event.stop_propagation();
            }
            return;
        }

        if let Some(region) = UiState(ctx).focused_sidebar_region()
            && self.handle_sidebar_key(event, modifiers, region)
        {
            event.prevent_default();
            event.stop_propagation();
            return;
        }

        if !event.repeat
            && let Some(command) = resolve_shortcut(event, modifiers)
            && (!command.needs_content_focus() || ctx.0.list_focused.get())
        {
            self.dispatch(command);
            event.prevent_default();
            event.stop_propagation();
            return;
        }

        if !event.repeat
            && let Some(index) = matching_ordinal_tab(event, modifiers)
        {
            let tab_count = ctx.read_untracked(|state| state.tabs.len());
            if index < tab_count {
                Navigator(ctx).select_tab(index);
                event.prevent_default();
                event.stop_propagation();
                return;
            }
        }

        if ctx.0.list_focused.get() && self.handle_content_key(event, modifiers) {
            event.prevent_default();
            event.stop_propagation();
        }
    }

    pub(in crate::pages::workspace) fn select_row(&self, index: usize, modifiers: Modifiers) {
        self.0.reset_typeahead();
        self.select_row_inner(index, modifiers);
    }

    fn select_row_from_typeahead(&self, index: usize) {
        self.select_row_inner(index, Modifiers::NONE);
        self.0.0.list_focus_visible.set(true);
        Rows(self.0).scroll_row_into_view(index);
    }

    fn select_row_inner(&self, index: usize, modifiers: Modifiers) {
        let changed = self.0.update(|state| {
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
            self.0.0.list_focus_visible.set(false);
            self.0.update_selection(|_| {});
        }
    }

    pub(in crate::pages::workspace) fn select_all(&self) {
        let changed = self.0.update(|state| {
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
            self.0.update_selection(|_| {});
        }
    }

    fn focus_sidebar_region(&self, region: SidebarRegion) {
        let ctx = self.0;
        if !ctx.0.sidebar.get_untracked() {
            ctx.0.sidebar.set(true);
        }
        if region == SidebarRegion::QuickAccess {
            ctx.0.sidebar_reveal_quick_access.set(true);
        }
        if region == SidebarRegion::Volumes {
            ctx.0.sidebar_reveal_volumes.set(true);
        }
        if ctx.0.sidebar_in_accordion.get_untracked()
            && let Some(section) = CollapsibleSection::from_region(region)
        {
            ctx.0
                .sidebar_accordion
                .set(AccordionSelection::Expanded(section));
        }
        let rows = self.sidebar_rows(region);
        let current = ctx.0.sidebar_cursor.get_untracked();
        let valid = current.as_ref().is_some_and(|item| rows.contains(item));
        if !valid {
            ctx.0.sidebar_cursor.set(rows.into_iter().next());
        }
        ctx.0.sidebar_region.set(Some(region));
        self.sidebar_node(region).focus();
        ctx.changed();
    }

    fn sidebar_rows(&self, region: SidebarRegion) -> Vec<SidebarRow> {
        let ctx = self.0;
        match region {
            SidebarRegion::Tabs => ctx.read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .map(|tab| SidebarRow::Tab(tab.id))
                    .collect()
            }),
            SidebarRegion::QuickAccess => QuickAccess(ctx)
                .quick_access_paths_untracked()
                .into_iter()
                .map(SidebarRow::QuickAccess)
                .collect(),
            SidebarRegion::Locations => {
                let tags_enabled = ctx.tags_enabled_untracked();
                LocationTarget::ALL
                    .into_iter()
                    .filter(|target| {
                        *target != LocationTarget::Collection(Collection::Tags) || tags_enabled
                    })
                    .map(SidebarRow::Location)
                    .collect()
            }
            SidebarRegion::Volumes => ctx
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
        let ctx = self.0;
        let rows = self.sidebar_rows(region);
        if rows.is_empty() {
            return false;
        }
        let current_idx = ctx
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
                ctx.0.sidebar_cursor.set(Some(rows[next].clone()));
                true
            }
            Key::Named(NamedKey::ArrowUp) => {
                let next = match current_idx {
                    Some(idx) => idx.saturating_sub(1),
                    None => rows.len() - 1,
                };
                ctx.0.sidebar_cursor.set(Some(rows[next].clone()));
                true
            }
            Key::Named(NamedKey::Home) => {
                ctx.0.sidebar_cursor.set(rows.into_iter().next());
                true
            }
            Key::Named(NamedKey::End) => {
                ctx.0.sidebar_cursor.set(rows.into_iter().last());
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
                if ctx.pane_untracked() == Pane::Files {
                    let _ = Lifecycle(ctx).cancel_clipboard_cut();
                }
                ctx.focus_content();
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
        let ctx = self.0;
        match row {
            SidebarRow::Tab(id) => {
                let nav = Navigator(ctx);
                if let Some(index) = nav.tab_index(id) {
                    nav.select_tab(index);
                    ctx.focus_content();
                }
            }
            SidebarRow::QuickAccess(path) => {
                QuickAccess(ctx).open_quick_access(&path, false);
                ctx.focus_content();
            }
            SidebarRow::Location(target) => {
                let home = ctx.read_untracked(|state| state.home.clone());
                Navigator(ctx).open(target.location(&home));
                ctx.focus_content();
            }
            SidebarRow::Volume(id) => {
                DeviceOps(ctx).open_volume(&id, false);
                ctx.focus_content();
            }
        }
    }

    fn handle_search_key(&self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        let ctx = self.0;
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
                        | Command::FocusQuickAccess
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
                if Geometry(ctx).height_tier_untracked() == gds::ui::window::HeightTier::Micro {
                    if !ctx.0.filter.get_untracked().is_empty() {
                        Navigator(ctx).filter(String::new());
                        ctx.0.filter_node.set_value("");
                    }
                    ctx.0.footer_filter_open.set(false);
                    ctx.focus_content();
                } else if !ctx.0.filter.get_untracked().is_empty() {
                    Navigator(ctx).filter(String::new());
                    ctx.0.filter_node.set_value("");
                } else {
                    ctx.0.footer_filter_open.set(false);
                    ctx.focus_content();
                }
                true
            }
            Key::Named(NamedKey::ArrowDown) => {
                ctx.focus_content();
                let row_count = ctx.read_untracked(|state| state.tab().rows.len());
                if row_count > 0 {
                    self.select_row(0, Modifiers::NONE);
                    ctx.0.list_focus_visible.set(true);
                    Rows(ctx).scroll_row_into_view(0);
                }
                true
            }
            Key::Named(NamedKey::ArrowUp) => {
                ctx.focus_content();
                let row_count = ctx.read_untracked(|state| state.tab().rows.len());
                if let Some(last) = row_count.checked_sub(1) {
                    self.select_row(last, Modifiers::NONE);
                    ctx.0.list_focus_visible.set(true);
                    Rows(ctx).scroll_row_into_view(last);
                }
                true
            }
            Key::Named(NamedKey::Enter) => {
                ctx.focus_content();
                let should_select_first = ctx.read_untracked(|state| {
                    let tab = state.tab();
                    !tab.rows.is_empty() && tab.navigation.view().selection.is_empty()
                });
                if should_select_first {
                    self.select_row(0, Modifiers::NONE);
                    ctx.0.list_focus_visible.set(true);
                    Rows(ctx).scroll_row_into_view(0);
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
        let content_id = self.0.0.content_focus.get_untracked();
        let list_id = self.0.0.list.get_untracked();
        content_id == Some(active) || list_id == Some(active) || self.0.0.list.contains(active)
    }

    fn can_handle_content_text_input(&self) -> bool {
        let ctx = self.0;
        ctx.pane_untracked() == Pane::Files
            && !ctx.0.menus.is_open()
            && !Inspection(ctx).has_open_inspection_dialog()
            && matches!(ctx.0.prompt.get_untracked(), Prompt::Closed)
            && !ctx.0.connector.is_connecting_untracked()
            && !gds::ui::keytips::has_active_modal_scope()
            && !TerminalDock(ctx).terminal_focused()
            && UiState(ctx).focused_sidebar_region().is_none()
            && self.content_surface_focused()
    }

    pub(in crate::pages::workspace) fn handle_content_text_input(
        &self,
        inserted: &str,
        now: Timestamp,
    ) -> bool {
        let ctx = self.0;
        if inserted.is_empty()
            || inserted.chars().all(char::is_control)
            || !self.can_handle_content_text_input()
        {
            return false;
        }
        if ctx.0.preferences.snapshot().files.auto_focus_search {
            ctx.reset_typeahead();
            if inserted.chars().all(char::is_whitespace)
                || !UiState(ctx).filter_available_untracked()
            {
                return false;
            }
            let mut next = ctx.0.filter.get_untracked();
            next.push_str(inserted);
            ctx.0.footer_filter_open.set(true);
            Navigator(ctx).filter(next.clone());
            if ctx.0.filter_node.is_bound() {
                ctx.0.filter_node.focus();
                ctx.0.filter_node.set_value(&next);
                ctx.0.filter_node.set_selection(next.len()..next.len());
            } else {
                ctx.0
                    .filter_focus_request
                    .set(Some(FilterFocusMode::CaretEnd));
            }
            return true;
        }
        self.typeahead_input(inserted, now)
    }

    fn typeahead_input(&self, inserted: &str, now: Timestamp) -> bool {
        let matched = {
            let state = self.0.0.state.borrow();
            let tab = state.tab();
            if matches!(tab.contents, Contents::Network { .. })
                || tab.navigation.is_loading()
                || tab.rows.is_empty()
            {
                return false;
            }
            let current = current_row_index(tab);
            let mut ta = self.0.0.typeahead.borrow_mut();
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
            let state = self.0.0.state.borrow();
            let tab = state.tab();
            if matches!(tab.contents, Contents::Network { .. })
                || tab.navigation.is_loading()
                || tab.rows.is_empty()
            {
                self.0.0.typeahead.borrow_mut().reset();
                return false;
            }
            let current = current_row_index(tab);
            self.0.0.typeahead.borrow_mut().pop_and_match(
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
        let ctx = self.0;
        let (count, mode, current_idx) = ctx.read_untracked(|state| {
            let tab = state.tab();
            (tab.rows.len(), tab.mode, current_row_index(tab))
        });

        if event.key == Key::Named(NamedKey::Enter) {
            ctx.reset_typeahead();
            if modifiers.shift() || modifiers.control() {
                MenuHost(ctx).open_selected_tab();
            } else {
                Dispatch(ctx).open_selected();
            }
            return true;
        }

        let no_command_mods = !modifiers.control() && !modifiers.alt() && !modifiers.meta();

        if event.key == Key::Named(NamedKey::Backspace)
            && no_command_mods
            && !ctx.0.preferences.snapshot().files.auto_focus_search
            && self.content_surface_focused()
            && self.typeahead_backspace(event.timestamp)
        {
            return true;
        }

        if event.key == Key::Named(NamedKey::Space) || event.key == Key::character(" ") {
            if no_command_mods
                && !ctx.0.preferences.snapshot().files.auto_focus_search
                && self.content_surface_focused()
                && ctx.0.typeahead.borrow().is_active(event.timestamp)
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
            ctx.0.list_focus_visible.set(true);
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
            ViewMode::Grid => Geometry(ctx).grid_columns_untracked().max(1),
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
            ctx.reset_typeahead();
            let moved = ctx.update(|state| {
                let tab = state.tab_mut();
                let Some(key) = tab.rows.get(target).map(Row::key) else {
                    return false;
                };
                tab.navigation.view_mut().cursor = Some(key);
                true
            });
            if moved {
                ctx.0.list_focus_visible.set(true);
                ctx.update_selection(|_| {});
                Rows(ctx).scroll_row_into_view(target);
            }
        } else {
            let sel_mod = if modifiers.shift() {
                Modifiers::SHIFT
            } else {
                Modifiers::NONE
            };
            self.select_row(target, sel_mod);
            ctx.0.list_focus_visible.set(true);
            Rows(ctx).scroll_row_into_view(target);
        }
        true
    }

    fn page_step_rows(&self, mode: ViewMode, columns: usize) -> usize {
        let scale = self.0.0.list.scale().max(1.0);
        let viewport_h = (self.0.0.list.scroll_position().scrollport.height.0 / scale).max(300.0);
        let row_h = Geometry(self.0).row_height_untracked().max(24.0);
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
