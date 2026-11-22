//! Navigation, tab management, location loading, filtering, sorting, and filesystem watching.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gds::service::task_manager::Owner as TaskOwner;
use gds::ui::IconName;
use tokio::sync::mpsc;
use zgui::geom::{DevicePx, Point};
use zgui::prelude::*;
use zgui::reactive::{on_cleanup_local, spawn_local};
use zgui::view::{ScrollBehavior, ScrollTarget};

use super::{
    model::{
        AddressPresentation, ConfirmOptions, Contents, InitialVisit, Row, RowKey, State, Tab,
        remote_task_key,
    },
    ui::Browser,
};
use crate::{
    entities::{
        file::{
            ViewMode,
            columns::Column,
            directory::{self, ContentType},
            sorting,
        },
        location::{
            Location, VolumeIdentity,
            navigation::{self, LoadOutcome, LoadRequest, PartialLoadOutcome},
            scheme, volumes,
        },
        tags,
    },
    features::{connect_server::Intent, file_operations::recycle},
    shared::{i18n, lib::selection::SelectionBacking},
};

pub(super) enum Loaded {
    Unavailable(String),
    Places(Vec<Location>),
    Network(Result<Vec<PathBuf>, String>),
    Trash(Vec<recycle::Entry>),
    Tags {
        listing: tags::Listing,
        backend: tags::Baloo,
        entries: Vec<directory::Entry>,
    },
    Volumes,
}

impl Browser {
    pub(super) fn resolve_path(&self, location: &Location) -> Option<PathBuf> {
        self.resolve_path_with(location, |identity| self.volume_at_location(identity))
    }

    pub(super) fn resolve_path_untracked(&self, location: &Location) -> Option<PathBuf> {
        self.resolve_path_with(location, |identity| {
            self.volume_at_location_untracked(identity)
        })
    }

    fn resolve_path_with(
        &self,
        location: &Location,
        find_volume: impl FnOnce(&VolumeIdentity) -> Option<volumes::Volume>,
    ) -> Option<PathBuf> {
        match location {
            Location::Directory(path) => Some(path.into()),
            Location::Volume { identity, relative } => {
                let volume = find_volume(identity)?;
                let preferred = volume.id.stable.as_ref().and_then(|id| {
                    self.0
                        .preferences
                        .snapshot()
                        .files
                        .volume_mount_points
                        .get(id)
                        .cloned()
                });
                preferred
                    .as_ref()
                    .filter(|preferred| volume.mount_points.contains(preferred))
                    .or_else(|| {
                        volume.mount_points.iter().min_by(|left, right| {
                            left.components().count().cmp(&right.components().count())
                        })
                    })
                    .map(|mount| {
                        if relative.as_os_str().is_empty() {
                            mount.clone()
                        } else {
                            mount.join(relative)
                        }
                    })
            }
            _ => None,
        }
    }

    pub(super) fn volume_at_location(&self, identity: &VolumeIdentity) -> Option<volumes::Volume> {
        Self::find_volume_in_snapshot(self.0.volumes.snapshot(), identity)
    }

    pub(super) fn volume_at_location_untracked(
        &self,
        identity: &VolumeIdentity,
    ) -> Option<volumes::Volume> {
        Self::find_volume_in_snapshot(self.0.volumes.snapshot_untracked(), identity)
    }

    fn find_volume_in_snapshot(
        snapshot: volumes::Snapshot,
        identity: &VolumeIdentity,
    ) -> Option<volumes::Volume> {
        snapshot.volumes.into_iter().find(|volume| match identity {
            VolumeIdentity::Stable(stable) => volume.id.stable.as_ref() == Some(stable),
            VolumeIdentity::Attachment(attachment) => volume.id.attachment == *attachment,
        })
    }

    pub(super) fn volume_identity(volume: &volumes::Volume) -> VolumeIdentity {
        if let Some(stable) = &volume.id.stable {
            VolumeIdentity::Stable(stable.clone())
        } else {
            VolumeIdentity::Attachment(volume.id.attachment.clone())
        }
    }

    pub(super) fn location_title(&self, location: &Location) -> String {
        match location {
            Location::Volume { identity, relative } if relative.is_empty() => self
                .volume_at_location_untracked(identity)
                .map_or_else(|| location.label(), |volume| volume.name),
            _ => location.label(),
        }
    }

    pub(super) fn location_address(location: &Location, _path: Option<&Path>) -> String {
        location.display_address()
    }

    pub(super) fn local_location(&self, path: &Path) -> Location {
        Location::from(path)
    }

    pub(super) fn place_entry(&self, location: &Location) -> Option<directory::Entry> {
        let path = self.resolve_path(location)?;
        let tab_id = self.read(|state| {
            let tab = state.tab();
            if let Contents::Tags { entries, .. } = &tab.contents
                && let Some(found) = entries.iter().find(|entry| entry.path == path)
            {
                return Err(found.clone());
            }
            Ok(tab.id)
        });
        match tab_id {
            Err(entry) => Some(entry),
            Ok(id) => {
                let key = (Rc::as_ptr(&self.0) as usize, id);
                PLACE_ENTRIES.with(|slot| {
                    slot.borrow()
                        .get(&key)
                        .and_then(|map| map.get(&path).cloned())
                })
            }
        }
    }

    pub(super) fn open(&self, location: Location) {
        let id = self.read_untracked(|state| state.tab().id);
        self.open_in_tab(id, location);
    }

    pub(super) fn open_in_tab(&self, id: u64, location: Location) {
        let in_picker = self.read_untracked(|state| state.picker.is_some());
        if in_picker && matches!(location, Location::Remote(_)) {
            self.report_notice(i18n::text_untracked("remote-picker-local"));
            self.sync_address();
            return;
        }
        if self.is_background_task_busy_untracked(&remote_task_key(id))
            && self.read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .is_some_and(|tab| matches!(tab.contents, Contents::Remote { .. }))
            })
        {
            self.report_notice(i18n::text_untracked("remote-task-running"));
            self.sync_address();
            return;
        }

        self.save_active_scroll_if(id);
        if self.read_untracked(|state| state.tabs.get(state.active).is_some_and(|tab| tab.id == id))
        {
            self.reset_typeahead();
        }
        self.0
            .address_presentation
            .set(AddressPresentation::Breadcrumbs);
        let resolved = self.resolve_path_untracked(&location);
        let show_hidden = self.0.preferences.snapshot().files.show_hidden;
        enum Dispatch {
            Remote(LoadRequest, gds::service::remote::Address),
            Standard(LoadRequest),
            UnavailableVolume { is_active: bool },
        }
        let dispatch = {
            let mut state = self.0.state.borrow_mut();
            let options = directory::ReadOptions { show_hidden };
            let is_active = state.tabs.get(state.active).is_some_and(|tab| tab.id == id);
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            tab.work = None;
            tab.enrichment = None;
            tab.reveal_hidden = false;
            tab.initial = InitialVisit::Explicit;
            if matches!(location, Location::Volume { .. }) && resolved.is_none() {
                tab.navigation.cancel_pending();
                tab.contents = Contents::Unavailable(i18n::text_untracked("device-not-mounted"));
                tab.rows.clear();
                tab.backing = SelectionBacking::from_items(std::iter::empty());
                Dispatch::UnavailableVolume { is_active }
            } else {
                let request = tab
                    .navigation
                    .prepare_open(location.clone(), resolved, options);
                match scheme::browse(&location) {
                    scheme::Browse::Remote(address) => Dispatch::Remote(request, address.clone()),
                    scheme::Browse::Location | scheme::Browse::Tags => Dispatch::Standard(request),
                }
            }
        };
        match dispatch {
            Dispatch::UnavailableVolume { is_active } => {
                self.clear_watch(id);
                if is_active {
                    self.0.filter.set(String::new());
                }
                self.sync_active_chrome();
                self.changed();
            }
            Dispatch::Remote(request, address) => {
                self.clear_watch(id);
                self.load_remote(id, request, address);
            }
            Dispatch::Standard(request) => self.start_load(id, request),
        }
    }

    pub(super) fn open_address(&self) {
        let raw = self.0.address.get_untracked();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            self.sync_address();
            return;
        }
        let target = if scheme::recognizes(trimmed) {
            scheme::parse(trimmed).map_err(|error| error.to_string())
        } else {
            let (current_dir, home) = self.read_untracked(|state| {
                let dir = state
                    .tab()
                    .navigation
                    .path()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| state.home.clone());
                (dir, state.home.clone())
            });
            navigation::resolve_input(trimmed, &current_dir, Some(&home))
                .map(|path| Location::from(path.as_path()))
                .map_err(|error| error.to_string())
        };
        match target {
            Ok(location) => {
                self.0
                    .address_presentation
                    .set(AddressPresentation::Breadcrumbs);
                self.open(location);
                self.focus_content();
            }
            Err(error) => {
                self.report_notice(error);
            }
        }
    }

    pub(super) fn refresh(&self) {
        let id = self.read_untracked(|state| state.tab().id);
        self.refresh_tab(id);
    }

    pub(super) fn reveal_paths(&self, paths: Vec<PathBuf>) {
        let active_id = self.read_untracked(|state| state.tab().id);
        self.update(|state| state.tab_mut().reveal = paths);
        self.reproject(active_id);
    }

    pub(super) fn refresh_tab(&self, id: u64) {
        self.save_active_scroll_if(id);
        let Some((location, in_picker)) = self.read_untracked(|state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| (tab.navigation.location().clone(), state.picker.is_some()))
        }) else {
            return;
        };
        if in_picker && matches!(location, Location::Remote(_)) {
            self.report_notice(i18n::text_untracked("remote-picker-local"));
            return;
        }
        let resolved = self.resolve_path_untracked(&location);
        let pref_show_hidden = self.0.preferences.snapshot().files.show_hidden;
        enum Dispatch {
            Remote(LoadRequest, gds::service::remote::Address),
            Standard(LoadRequest),
            UnavailableVolume,
        }
        let dispatch = {
            let mut state = self.0.state.borrow_mut();
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            let options = directory::ReadOptions {
                show_hidden: pref_show_hidden || tab.reveal_hidden,
            };
            tab.work = None;
            tab.enrichment = None;
            if matches!(location, Location::Volume { .. }) && resolved.is_none() {
                tab.navigation.cancel_pending();
                tab.contents = Contents::Unavailable(i18n::text_untracked("device-not-mounted"));
                tab.rows.clear();
                tab.backing = SelectionBacking::from_items(std::iter::empty());
                Dispatch::UnavailableVolume
            } else {
                let request = if matches!(tab.initial, InitialVisit::Explicit)
                    && tab.navigation.directory().is_none()
                    && tab.rows.is_empty()
                    && matches!(tab.contents, Contents::Directory)
                {
                    tab.initial = InitialVisit::Restored;
                    tab.navigation.prepare_initial(resolved, options, true)
                } else {
                    tab.navigation.prepare_refresh(resolved, options)
                };
                match scheme::browse(&location) {
                    scheme::Browse::Remote(address) => Dispatch::Remote(request, address.clone()),
                    scheme::Browse::Location | scheme::Browse::Tags => Dispatch::Standard(request),
                }
            }
        };
        match dispatch {
            Dispatch::UnavailableVolume => {
                self.clear_watch(id);
                self.changed();
            }
            Dispatch::Remote(request, address) => {
                self.clear_watch(id);
                self.load_remote(id, request, address);
            }
            Dispatch::Standard(request) => self.start_load(id, request),
        }
    }

    pub(super) fn history(&self, forward: bool) {
        let target = self.read_untracked(|state| {
            let tab = state.tab();
            let index = tab.navigation.history_index();
            let target_index = if forward {
                index.checked_add(1)
            } else {
                index.checked_sub(1)
            }?;
            let visit = tab.navigation.history().get(target_index)?;
            Some((tab.id, target_index, visit.location.clone()))
        });
        if let Some((id, index, location)) = target {
            self.visit_history(id, index, &location);
        }
    }

    pub(super) fn visit_history(&self, id: u64, index: usize, location: &Location) {
        let in_picker = self.read_untracked(|state| state.picker.is_some());
        if in_picker && matches!(location, Location::Remote(_)) {
            self.report_notice(i18n::text_untracked("remote-picker-local"));
            return;
        }
        if self.is_background_task_busy_untracked(&remote_task_key(id))
            && self.read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .is_some_and(|tab| matches!(tab.contents, Contents::Remote { .. }))
            })
        {
            self.report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }

        self.save_active_scroll_if(id);
        if self.read_untracked(|state| state.tabs.get(state.active).is_some_and(|tab| tab.id == id))
        {
            self.reset_typeahead();
        }
        self.0
            .address_presentation
            .set(AddressPresentation::Breadcrumbs);
        let resolved = self.resolve_path_untracked(location);
        let pref_show_hidden = self.0.preferences.snapshot().files.show_hidden;
        enum Dispatch {
            Remote(LoadRequest, gds::service::remote::Address),
            Standard(LoadRequest),
            UnavailableVolume,
            Noop,
        }
        let dispatch = {
            let mut state = self.0.state.borrow_mut();
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            let options = directory::ReadOptions {
                show_hidden: pref_show_hidden || tab.reveal_hidden,
            };
            if matches!(location, Location::Volume { .. }) && resolved.is_none() {
                tab.work = None;
                tab.enrichment = None;
                tab.navigation.cancel_pending();
                tab.contents = Contents::Unavailable(i18n::text_untracked("device-not-mounted"));
                tab.rows.clear();
                tab.backing = SelectionBacking::from_items(std::iter::empty());
                Dispatch::UnavailableVolume
            } else if let Some(request) = tab.navigation.prepare_history(index, resolved, options) {
                tab.work = None;
                tab.enrichment = None;
                match scheme::browse(location) {
                    scheme::Browse::Remote(address) => Dispatch::Remote(request, address.clone()),
                    scheme::Browse::Location | scheme::Browse::Tags => Dispatch::Standard(request),
                }
            } else {
                Dispatch::Noop
            }
        };
        match dispatch {
            Dispatch::Noop => {}
            Dispatch::UnavailableVolume => {
                self.clear_watch(id);
                self.sync_active_chrome();
                self.changed();
            }
            Dispatch::Remote(request, address) => {
                self.clear_watch(id);
                self.load_remote(id, request, address);
            }
            Dispatch::Standard(request) => self.start_load(id, request),
        }
    }

    pub(super) fn cancel_location_load(&self) {
        let (tab_id, unopened_remote) = self.update(|state| {
            let tab = state.tab_mut();
            tab.navigation.cancel_pending();
            tab.work = None;
            let unopened_remote = matches!(tab.navigation.location(), Location::Remote(_))
                && !matches!(tab.contents, Contents::Remote { .. });
            (tab.id, unopened_remote)
        });
        if self
            .0
            .connector
            .pending_untracked()
            .is_some_and(|(_, intent)| intent == Intent::Open)
        {
            self.0.connector.cancel();
        }
        if unopened_remote {
            self.open_in_tab(tab_id, Location::Network);
        } else {
            self.sync_address();
            self.changed();
        }
    }

    pub(super) fn parent(&self) {
        let parent = self.read_untracked(|state| scheme::parent(state.tab().navigation.location()));
        if let Some(location) = parent {
            self.open(location);
        }
    }

    pub(super) fn new_tab(&self, location: Option<Location>) {
        let in_picker = self.read_untracked(|state| state.picker.is_some());
        if in_picker && matches!(location, Some(Location::Remote(_))) {
            self.report_notice(i18n::text_untracked("remote-picker-local"));
            return;
        }
        self.save_active_scroll();
        let new_id = {
            let mut state = self.0.state.borrow_mut();
            let target = location.unwrap_or_else(|| Location::from(state.home.as_path()));
            let id = state.next_tab;
            state.next_tab += 1;
            let insert_at = (state.active + 1).min(state.tabs.len());
            let pinned_end = state
                .tabs
                .iter()
                .position(|tab| !tab.pinned)
                .unwrap_or(state.tabs.len());
            let index = insert_at.max(pinned_end);
            let tab = Tab::new(id, target, &state.places, false);
            state.tabs.insert(index, tab);
            state.active = index;
            id
        };
        self.sync_active_chrome();
        self.changed();
        self.refresh_tab(new_id);
        self.focus_content();
    }

    pub(super) fn select_tab(&self, index: usize) {
        self.save_active_scroll();
        let (changed, needs_hidden_refresh) = self.update(|state| {
            if index < state.tabs.len() && state.active != index {
                state.active = index;
                let tab = state.tab();
                let pref_hidden = self.0.preferences.snapshot().files.show_hidden;
                let mismatch = tab.navigation.directory().is_some_and(|dir| {
                    dir.options.show_hidden != (pref_hidden || tab.reveal_hidden)
                });
                (true, mismatch.then_some(tab.id))
            } else {
                (false, None)
            }
        });
        if changed {
            self.0
                .address_presentation
                .set(AddressPresentation::Breadcrumbs);
            self.sync_active_chrome();
            self.update_selection(|_| {});
            self.restore_active_scroll();
            if let Some(id) = needs_hidden_refresh {
                self.refresh_tab(id);
            }
        }
    }

    pub(super) fn tab_index(&self, id: u64) -> Option<usize> {
        self.read_untracked(|state| state.tabs.iter().position(|tab| tab.id == id))
    }

    pub(super) fn close_tab(&self, index: usize) {
        let target = self.read_untracked(|state| {
            let tab = state.tabs.get(index)?;
            let loc = tab.navigation.location();
            let title = match loc {
                Location::Volume { identity, relative } if relative.is_empty() => self
                    .volume_at_location_untracked(identity)
                    .map_or_else(|| loc.label(), |v| v.name),
                _ => loc.label(),
            };
            Some((tab.id, tab.pinned, title))
        });
        let Some((id, pinned, title)) = target else {
            return;
        };
        if pinned {
            let confirm_title = i18n::text_untracked("unpin-close-title");
            let message = gds::t_untracked!("unpin-close-message", name = title);
            let primary = i18n::text_untracked("unpin-close-confirm");
            let options = ConfirmOptions {
                icon: Some(IconName::Pin),
                primary_icon: Some(IconName::X),
                destructive: false,
                items: Vec::new(),
                overflow_count: 0,
            };
            let browser = self.clone();
            self.confirm_with_options(confirm_title, message, primary, options, move || {
                browser.close_tab_approved(id);
            });
        } else {
            self.close_tab_approved(id);
        }
    }

    fn close_tab_approved(&self, id: u64) {
        struct RemovedOutcome {
            removed_id: u64,
            need_save_pins: bool,
            reopened_home_id: Option<u64>,
        }
        let outcome = {
            let mut state = self.0.state.borrow_mut();
            let Some(index) = state.tabs.iter().position(|tab| tab.id == id) else {
                return;
            };
            let removed = state.tabs.remove(index);
            let removed_loc = removed.navigation.location().clone();
            let was_pinned = removed.pinned;
            if was_pinned {
                let still_pinned_same = state
                    .tabs
                    .iter()
                    .any(|tab| tab.pinned && tab.navigation.location() == &removed_loc);
                if !still_pinned_same {
                    state.places.set_pinned(&removed_loc, false);
                }
                let pinned_order = pinned_tab_locations(&state.tabs);
                state.places.reorder_pinned(pinned_order);
            }

            let reopened_home_id = if state.tabs.is_empty() {
                let home_id = state.next_tab;
                state.next_tab += 1;
                let home_loc = Location::from(state.home.as_path());
                let home_tab = Tab::new(home_id, home_loc, &state.places, false);
                state.tabs.push(home_tab);
                state.active = 0;
                Some(home_id)
            } else {
                if state.active >= state.tabs.len() {
                    state.active = state.tabs.len() - 1;
                } else if index < state.active {
                    state.active -= 1;
                }
                None
            };

            RemovedOutcome {
                removed_id: removed.id,
                need_save_pins: was_pinned,
                reopened_home_id,
            }
        };

        self.clear_watch(outcome.removed_id);
        let _ = self.finish_background_task(&remote_task_key(outcome.removed_id));
        if outcome.need_save_pins {
            self.save_places();
        }
        self.0
            .address_presentation
            .set(AddressPresentation::Breadcrumbs);
        self.sync_active_chrome();
        self.update_selection(|_| {});
        self.changed();
        if let Some(home_id) = outcome.reopened_home_id {
            self.refresh_tab(home_id);
        } else {
            self.restore_active_scroll();
        }
        self.focus_content();
    }

    pub(super) fn toggle_pin(&self, index: usize) {
        let changed = self.update(|state| {
            let result = apply_toggle_pin(&mut state.tabs, &mut state.active, index);
            if let Some((loc, now_pinned)) = result {
                let still_pinned_same = state
                    .tabs
                    .iter()
                    .any(|tab| tab.pinned && tab.navigation.location() == &loc);
                state
                    .places
                    .set_pinned(&loc, now_pinned || still_pinned_same);
                let order = pinned_tab_locations(&state.tabs);
                state.places.reorder_pinned(order);
                true
            } else {
                false
            }
        });
        if changed {
            self.save_places();
            self.changed();
        }
    }

    pub(super) fn reorder_tab(&self, from: usize, to: usize) {
        let (changed, moved_pinned) = self.update(|state| {
            let Some(was_pinned) = apply_reorder_tab(&mut state.tabs, &mut state.active, from, to)
            else {
                return (false, false);
            };
            if was_pinned {
                let order = pinned_tab_locations(&state.tabs);
                state.places.reorder_pinned(order);
            }
            (true, was_pinned)
        });
        if changed {
            if moved_pinned {
                self.save_places();
            }
            self.changed();
        }
    }

    pub(super) fn sortable_columns(&self) -> Vec<Column> {
        self.read(sortable_columns_for)
    }

    pub(super) fn sortable_columns_untracked(&self) -> Vec<Column> {
        self.read_untracked(sortable_columns_for)
    }

    pub(super) fn reproject(&self, id: u64) {
        let volumes_snapshot = self.0.volumes.snapshot_untracked().volumes;
        let pref_show_hidden = self.0.preferences.snapshot().files.show_hidden;
        let reveal_scroll_idx = {
            let mut state = self.0.state.borrow_mut();
            let in_picker = state.picker.is_some();
            let picker_projection = state.picker.as_ref().map(|picker| picker.projection());
            let is_active = state.tabs.get(state.active).is_some_and(|tab| tab.id == id);
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            if !location_supports_grid(tab.navigation.location()) {
                tab.mode = ViewMode::List;
            }
            let show_hidden = pref_show_hidden || tab.reveal_hidden;
            let filter_lower = tab.navigation.view().filter.trim().to_lowercase();
            let sort = tab.sort;
            let rows: Vec<Row> = match &tab.contents {
                Contents::Directory => {
                    let Some(snapshot) = tab.navigation.directory() else {
                        tab.rows.clear();
                        tab.backing = SelectionBacking::from_items(std::iter::empty());
                        return;
                    };
                    let mut indices: Vec<usize> = snapshot
                        .entries
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, entry)| {
                            if let Some(projection) = picker_projection.as_ref()
                                && !projection.keeps(entry)
                            {
                                return None;
                            }
                            if !filter_lower.is_empty() && !entry.matches_filter(&filter_lower) {
                                return None;
                            }
                            Some(idx)
                        })
                        .collect();
                    sort.sort_indices(&snapshot.entries, &mut indices);
                    indices
                        .into_iter()
                        .map(|idx| Row::File(snapshot.entries[idx].clone()))
                        .collect()
                }
                Contents::Tags {
                    listing, entries, ..
                } => {
                    let mut out: Vec<Row> = listing
                        .tags
                        .iter()
                        .filter(|tag| {
                            filter_lower.is_empty() || tag.to_lowercase().contains(&filter_lower)
                        })
                        .cloned()
                        .map(Row::Tag)
                        .collect();
                    let mut indices: Vec<usize> = entries
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, entry)| {
                            if let Some(projection) = picker_projection.as_ref()
                                && !projection.keeps(entry)
                            {
                                return None;
                            }
                            if !filter_lower.is_empty() && !entry.matches_filter(&filter_lower) {
                                return None;
                            }
                            Some(idx)
                        })
                        .collect();
                    sort.sort_indices(entries, &mut indices);
                    out.extend(
                        indices
                            .into_iter()
                            .map(|idx| Row::File(entries[idx].clone())),
                    );
                    out
                }
                Contents::Places(locations) => locations
                    .iter()
                    .filter(|loc| !in_picker || !matches!(loc, Location::Remote(_)))
                    .filter(|loc| {
                        filter_lower.is_empty()
                            || loc.label().to_lowercase().contains(&filter_lower)
                            || loc.display_address().to_lowercase().contains(&filter_lower)
                    })
                    .cloned()
                    .map(Row::Place)
                    .collect(),
                Contents::Network { mounts, .. } => mounts
                    .iter()
                    .filter(|path| {
                        filter_lower.is_empty()
                            || path
                                .to_string_lossy()
                                .to_lowercase()
                                .contains(&filter_lower)
                    })
                    .map(|path| Row::Place(Location::from(path.as_path())))
                    .collect(),
                Contents::Trash(entries) => entries
                    .iter()
                    .filter(|entry| {
                        filter_lower.is_empty()
                            || entry
                                .name
                                .to_string_lossy()
                                .to_lowercase()
                                .contains(&filter_lower)
                            || entry
                                .original_path
                                .to_string_lossy()
                                .to_lowercase()
                                .contains(&filter_lower)
                    })
                    .cloned()
                    .map(Row::Trash)
                    .collect(),
                Contents::Remote { listing, .. } => {
                    let mut filtered =
                        filter_remote_entries(&listing.entries, &filter_lower, show_hidden);
                    sort_remote_entries(&mut filtered, sort);
                    filtered.into_iter().map(Row::Remote).collect()
                }
                Contents::Volumes => volumes_snapshot
                    .iter()
                    .filter(|volume| {
                        volume.visible
                            && (filter_lower.is_empty()
                                || volume.name.to_lowercase().contains(&filter_lower)
                                || volume.label.to_lowercase().contains(&filter_lower))
                    })
                    .cloned()
                    .map(Row::Volume)
                    .collect(),
                Contents::LoadingCollection | Contents::Unavailable(_) => Vec::new(),
            };

            tab.backing = SelectionBacking::from_items(rows.iter().map(Row::key));
            tab.rows = rows;

            let mut scroll_to_reveal = None;
            if !tab.reveal.is_empty() {
                let mut revealed_keys = Vec::new();
                for (row_idx, row) in tab.rows.iter().enumerate() {
                    if let Some(path) = row.path()
                        && tab.reveal.iter().any(|target| target == path)
                    {
                        if scroll_to_reveal.is_none() {
                            scroll_to_reveal = Some(row_idx);
                        }
                        revealed_keys.push(row.key());
                    }
                }
                if !revealed_keys.is_empty() {
                    let first = revealed_keys[0].clone();
                    let view = tab.navigation.view_mut();
                    view.selection.replace(revealed_keys);
                    view.anchor = Some(first.clone());
                    view.cursor = Some(first);
                    tab.reveal.clear();
                }
            }
            let backing = Arc::clone(&tab.backing);
            let view = tab.navigation.view_mut();
            view.selection.retain(|key| backing.position(key).is_some());
            if view
                .cursor
                .as_ref()
                .is_some_and(|cursor| backing.position(cursor).is_none())
            {
                view.cursor = None;
            }
            if view
                .anchor
                .as_ref()
                .is_some_and(|anchor| backing.position(anchor).is_none())
            {
                view.anchor = None;
            }
            if is_active { scroll_to_reveal } else { None }
        };

        self.changed();
        if self.read_untracked(|state| state.tabs.get(state.active).is_some_and(|tab| tab.id == id))
        {
            self.update_selection(|_| {});
            self.sync_address();
            if let Some(row_idx) = reveal_scroll_idx {
                self.scroll_row_into_view(row_idx);
            }
        }
    }

    pub(super) fn sync_address(&self) {
        let address = self.read_untracked(|state| {
            let tab = state.tab();
            tab.navigation
                .pending_location()
                .unwrap_or_else(|| tab.navigation.location())
                .display_address()
        });
        self.0.address.set(address);
    }

    pub(super) fn filter(&self, value: String) {
        self.reset_typeahead();
        let id = {
            let mut state = self.0.state.borrow_mut();
            let tab = state.tab_mut();
            if tab.navigation.view().filter == value {
                return;
            }
            tab.navigation.view_mut().filter = value.clone();
            tab.id
        };
        self.0.filter.set(value);
        self.reproject(id);
        self.enrich_directory(id);
    }

    pub(super) fn toggle_view(&self) {
        let Some(id) = ({
            let mut state = self.0.state.borrow_mut();
            let tab = state.tab_mut();
            if !location_supports_grid(tab.navigation.location()) {
                None
            } else {
                let next_mode = match tab.mode {
                    ViewMode::List => ViewMode::Grid,
                    ViewMode::Grid => ViewMode::List,
                };
                tab.mode = next_mode;
                let loc = tab.navigation.location().clone();
                let id = tab.id;
                state.places.set_view_mode(loc, next_mode);
                Some(id)
            }
        }) else {
            return;
        };
        self.save_places();
        self.changed();
        self.enrich_directory(id);
    }

    pub(super) fn favorite(&self) {
        let (mutated, selection_cleared) = {
            let mut state = self.0.state.borrow_mut();
            toggle_tab_or_selected_favorite(&mut state)
        };
        if !mutated {
            return;
        }
        self.save_places();
        self.refresh_favorites_tabs();
        if selection_cleared {
            self.update_selection(|_| {});
        }
        self.changed();
    }

    pub(super) fn scroll_row_into_view(&self, index: usize) {
        let row_h = self.row_height_untracked().max(1.0);
        let grid = self.grid_geometry_untracked();
        let scale = self.0.list.scale().max(1.0);
        let mode = self.read_untracked(|state| state.tab().mode);
        let (item_top, item_bottom) = match mode {
            ViewMode::List => {
                let top = index as f32 * row_h;
                (top, top + row_h)
            }
            ViewMode::Grid => {
                let rect = grid.tile(index, scale);
                (rect.y, rect.y + rect.height)
            }
        };
        let pos = self.0.list.scroll_position();
        let cur_top = pos.offset.y.0 / scale;
        let viewport_h = pos.scrollport.height.0 / scale;
        if viewport_h <= 0.0 {
            return;
        }
        let target_top = if item_top < cur_top {
            Some(item_top)
        } else if item_bottom > cur_top + viewport_h {
            Some((item_bottom - viewport_h).max(0.0))
        } else {
            None
        };
        if let Some(top) = target_top {
            self.0.list.scroll_to(
                ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(top * scale))),
                ScrollBehavior::Instant,
            );
        }
    }

    fn save_active_scroll(&self) {
        let scale = self.0.list.scale().max(1.0);
        let top = (self.0.list.scroll_offset().y.0 / scale).max(0.0);
        self.update(|state| {
            state.tab_mut().navigation.view_mut().scroll = top;
        });
    }

    fn save_active_scroll_if(&self, id: u64) {
        let is_active = self.read_untracked(|state| state.tab().id == id);
        if is_active {
            self.save_active_scroll();
        }
    }

    pub(super) fn restore_active_scroll(&self) {
        let top = self.read_untracked(|state| state.tab().navigation.view().scroll.max(0.0));
        let scale = self.0.list.scale().max(1.0);
        self.0.list.scroll_to(
            ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(top * scale))),
            ScrollBehavior::Instant,
        );
    }

    fn sync_active_chrome(&self) {
        self.reset_typeahead();
        let (address, filter, details, widths) = self.read_untracked(|state| {
            let tab = state.tab();
            let loc = tab.navigation.location();
            let widths: HashMap<Column, f32> = state
                .places
                .column_widths(loc)
                .into_iter()
                .filter_map(|(id, width)| Column::from_id(&id).map(|col| (col, width)))
                .collect();
            (
                loc.display_address(),
                tab.navigation.view().filter.clone(),
                state.places.details_visible(loc),
                widths,
            )
        });
        self.0.address.set(address);
        self.0.filter.set(filter);
        self.0.details.set(details);
        self.0.column_widths.set(widths);
    }

    pub(super) fn apply_location_view_settings(&self, id: u64, location: &Location) {
        let is_active = {
            let mut state = self.0.state.borrow_mut();
            let is_active = state.tabs.get(state.active).is_some_and(|tab| tab.id == id);
            let mode = if location_supports_grid(location) {
                state.places.effective_view_mode(location)
            } else {
                ViewMode::List
            };
            let sort = state.places.effective_sort(location);
            let columns = state.places.effective_columns(location).clone();
            if let Some(tab) = state.tab_by_id(id) {
                tab.mode = mode;
                tab.sort = sort;
                tab.columns = columns;
            }
            is_active
        };
        if is_active {
            self.sync_active_chrome();
        }
    }

    pub(super) fn refresh_favorites_tabs(&self) {
        let ids: Vec<u64> = self.read_untracked(|state| {
            state
                .tabs
                .iter()
                .filter(|tab| matches!(tab.navigation.location(), Location::Favorites))
                .map(|tab| tab.id)
                .collect()
        });
        for id in ids {
            self.refresh_tab(id);
        }
    }

    fn inspect_places_metadata(&self, id: u64, locations: &[Location]) {
        let local_paths: Vec<PathBuf> = locations
            .iter()
            .filter_map(|loc| self.resolve_path_untracked(loc))
            .collect();
        let window_key = Rc::as_ptr(&self.0) as usize;
        if local_paths.is_empty() {
            PLACE_ENTRIES.with(|slot| {
                slot.borrow_mut().remove(&(window_key, id));
            });
            return;
        }
        ensure_watch_cleanup(self);
        let work = self.0.tasks.named_child("Inspect place metadata");
        let scope = (*work).clone();
        let cancel = work.token();
        {
            let mut state = self.0.state.borrow_mut();
            if let Some(tab) = state.tab_by_id(id) {
                tab.work = Some(work);
            } else {
                return;
            }
        }
        let (tx, mut rx) = mpsc::unbounded_channel();
        drop(
            scope.spawn_blocking_named("Inspect place entries", move || {
                let mut map = HashMap::with_capacity(local_paths.len());
                for path in local_paths {
                    if cancel.is_cancelled() {
                        return;
                    }
                    if let Ok(entry) = directory::Entry::inspect(&path) {
                        map.insert(path, entry);
                    }
                }
                let _ = tx.send(map);
            }),
        );
        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let Some(entries) = rx.recv().await else {
                    return;
                };
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                let still_places = browser.read_untracked(|state| {
                    state
                        .tabs
                        .iter()
                        .find(|tab| tab.id == id)
                        .is_some_and(|tab| matches!(tab.contents, Contents::Places(_)))
                });
                if !still_places {
                    return;
                }
                PLACE_ENTRIES.with(|slot| {
                    slot.borrow_mut().insert((window_key, id), entries);
                });
                browser.update(|state| {
                    if let Some(tab) = state.tab_by_id(id) {
                        tab.work = None;
                    }
                });
                browser.changed();
            }));
        });
    }

    fn start_load(&self, id: u64, request: LoadRequest) {
        self.clear_watch(id);
        let location = request.location().clone();
        if request.path().is_some() {
            self.start_directory_load(id, request);
            return;
        }
        match location {
            Location::Recent => {
                let places = self.read_untracked(|state| state.places.recent.clone());
                self.finish_collection_load(id, &request, Loaded::Places(places.clone()));
                self.inspect_places_metadata(id, &places);
            }
            Location::Favorites => {
                let places = self.read_untracked(|state| state.places.favorites.clone());
                self.finish_collection_load(id, &request, Loaded::Places(places.clone()));
                self.inspect_places_metadata(id, &places);
            }
            Location::Volumes => {
                self.0.volumes.refresh();
                self.finish_collection_load(id, &request, Loaded::Volumes);
            }
            Location::Network => {
                let work = self.0.tasks.named_child("Load network mounts");
                let scope = (*work).clone();
                let is_active = self.read_untracked(|state| state.tab().id == id);
                {
                    let mut state = self.0.state.borrow_mut();
                    if let Some(tab) = state.tab_by_id(id) {
                        if !matches!(tab.contents, Contents::Network { .. }) {
                            tab.contents = Contents::LoadingCollection;
                            tab.rows.clear();
                        }
                        tab.work = Some(work);
                    }
                }
                if is_active {
                    self.sync_address();
                }
                self.changed();
                let (tx, mut rx) = mpsc::unbounded_channel();
                drop(scope.spawn_blocking_named("Read network mounts", move || {
                    let _ = tx.send(volumes::read_network_mounts());
                }));
                let weak = self.downgrade();
                self.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let Some(result) = rx.recv().await else {
                            return;
                        };
                        let Some(browser) = weak.upgrade() else {
                            return;
                        };
                        let loaded = Loaded::Network(result.map_err(|error| error.to_string()));
                        browser.finish_collection_load(id, &request, loaded);
                    }));
                });
            }
            Location::Trash => {
                let work = self.0.tasks.named_child("Load Trash");
                let scope = (*work).clone();
                {
                    let mut state = self.0.state.borrow_mut();
                    if let Some(tab) = state.tab_by_id(id) {
                        if !matches!(tab.contents, Contents::Trash(_)) {
                            tab.contents = Contents::LoadingCollection;
                            tab.rows.clear();
                        }
                        tab.work = Some(work);
                    }
                }
                self.changed();
                let (tx, mut rx) = mpsc::unbounded_channel();
                drop(scope.spawn_blocking_named("List Trash", move || {
                    let _ = tx.send(recycle::list());
                }));
                let weak = self.downgrade();
                self.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let Some(result) = rx.recv().await else {
                            return;
                        };
                        let Some(browser) = weak.upgrade() else {
                            return;
                        };
                        let loaded = match result {
                            Ok(entries) => Loaded::Trash(entries),
                            Err(error) => Loaded::Unavailable(error.to_string()),
                        };
                        browser.finish_collection_load(id, &request, loaded);
                    }));
                });
            }
            Location::Tags(tag_addr) => {
                let tag_prefs = self.0.preferences.snapshot().tags;
                if !tag_prefs.enabled {
                    self.finish_collection_load(
                        id,
                        &request,
                        Loaded::Unavailable(i18n::text_untracked("tags-unavailable")),
                    );
                    return;
                }
                let work = self.0.tasks.named_child("Load tags");
                let scope = (*work).clone();
                let cancel = work.token();
                {
                    let mut state = self.0.state.borrow_mut();
                    if let Some(tab) = state.tab_by_id(id) {
                        if !matches!(tab.contents, Contents::Tags { .. }) {
                            tab.contents = Contents::LoadingCollection;
                            tab.rows.clear();
                        }
                        tab.work = Some(work);
                    }
                }
                self.changed();
                let (tx, mut rx) = mpsc::unbounded_channel();
                let tag = tag_addr.tag().to_owned();
                let commands = tag_prefs.commands;
                let blocking_scope = scope.clone();
                drop(scope.spawn_named("Browse tags", async move {
                    let outcome = async {
                        let backend = tags::connect(&commands).await?;
                        let listing = backend.browse(&tag).await.map_err(|e| e.to_string())?;
                        let files = listing.files.clone();
                        let entries = blocking_scope
                            .spawn_blocking_named("Inspect tagged files", move || {
                                let mut entries = Vec::with_capacity(files.len());
                                for path in files {
                                    if cancel.is_cancelled() {
                                        break;
                                    }
                                    if let Ok(entry) = directory::Entry::inspect(&path) {
                                        entries.push(entry);
                                    }
                                }
                                entries
                            })
                            .await
                            .unwrap_or_default();
                        Ok::<_, String>((listing, backend, entries))
                    }
                    .await;
                    let _ = tx.send(outcome);
                }));
                let weak = self.downgrade();
                self.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let Some(result) = rx.recv().await else {
                            return;
                        };
                        let Some(browser) = weak.upgrade() else {
                            return;
                        };
                        let loaded = match result {
                            Ok((listing, backend, entries)) => Loaded::Tags {
                                listing,
                                backend,
                                entries,
                            },
                            Err(error) => Loaded::Unavailable(error),
                        };
                        browser.finish_collection_load(id, &request, loaded);
                    }));
                });
            }
            Location::Directory(_) | Location::Volume { .. } => {
                self.finish_collection_load(
                    id,
                    &request,
                    Loaded::Unavailable(i18n::text_untracked("device-not-mounted")),
                );
            }
            Location::Remote(address) => {
                self.load_remote(id, request, address);
            }
        }
    }

    fn start_directory_load(&self, id: u64, request: LoadRequest) {
        let stream_options = self.0.preferences.snapshot().files.streaming.options();
        let (scope, cancel) = {
            let mut state = self.0.state.borrow_mut();
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            let work = self.0.tasks.named_child("Load directory");
            let scope = (*work).clone();
            let cancel = work.token();
            tab.work = Some(work);
            (scope, cancel)
        };
        enum DirLoadMsg {
            Partial(Arc<directory::DirectorySnapshot>),
            Done(Result<Option<Arc<directory::DirectorySnapshot>>, directory::Error>),
        }
        let (tx, mut rx) = mpsc::unbounded_channel();
        let req = request.clone();
        drop(scope.spawn_blocking_named("Read directory", move || {
            let progress_tx = tx.clone();
            let result = req.load_base_streaming(stream_options, &cancel, move |snapshot| {
                let _ = progress_tx.send(DirLoadMsg::Partial(snapshot));
            });
            let _ = tx.send(DirLoadMsg::Done(result));
        }));

        let weak = self.downgrade();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                while let Some(msg) = rx.recv().await {
                    let Some(browser) = weak.upgrade() else {
                        break;
                    };
                    match msg {
                        DirLoadMsg::Partial(snapshot) => {
                            let mut pin_saved = false;
                            let outcome = {
                                let mut state = browser.0.state.borrow_mut();
                                let Some(tab) = state.tab_by_id(id) else {
                                    break;
                                };
                                let prior_pin =
                                    tab.pinned.then(|| tab.navigation.location().clone());
                                let outcome = tab.navigation.apply_partial_load(&request, snapshot);
                                if outcome != PartialLoadOutcome::Stale {
                                    tab.contents = Contents::Directory;
                                }
                                if outcome == PartialLoadOutcome::FirstCommit
                                    && let Some(old_loc) = prior_pin
                                {
                                    let new_loc = tab.navigation.location().clone();
                                    pin_saved =
                                        sync_pinned_navigation(&mut state, &old_loc, &new_loc);
                                }
                                outcome
                            };
                            if pin_saved {
                                browser.save_places();
                            }
                            match outcome {
                                PartialLoadOutcome::FirstCommit => {
                                    browser.apply_location_view_settings(id, request.location());
                                    browser.reproject(id);
                                }
                                PartialLoadOutcome::Updated => {
                                    browser.reproject(id);
                                }
                                PartialLoadOutcome::Stale => break,
                            }
                        }
                        DirLoadMsg::Done(result) => {
                            let mut need_save = false;
                            let finished = {
                                let mut state = browser.0.state.borrow_mut();
                                let Some(tab) = state.tab_by_id(id) else {
                                    break;
                                };
                                let prior_pin =
                                    tab.pinned.then(|| tab.navigation.location().clone());
                                let finish = tab.navigation.finish_load(&request, result);
                                match finish {
                                    Ok(LoadOutcome::Stale) => None,
                                    Ok(LoadOutcome::Applied) => {
                                        if let Some(snapshot) = tab.navigation.directory() {
                                            for warning in &snapshot.warnings {
                                                tracing::warn!(
                                                    path = %warning.path.display(),
                                                    operation = ?warning.operation,
                                                    error = %warning.source,
                                                    "directory scan entry warning"
                                                );
                                            }
                                        }
                                        tab.work = None;
                                        tab.contents = Contents::Directory;
                                        let loc = tab.navigation.location().clone();
                                        if request.records_recent() {
                                            state.places.record(loc.clone());
                                            need_save = true;
                                        }
                                        if let Some(old_loc) = prior_pin
                                            && sync_pinned_navigation(&mut state, &old_loc, &loc)
                                        {
                                            need_save = true;
                                        }
                                        Some(Ok(loc))
                                    }
                                    Err(error) => {
                                        if error.is_cancelled() {
                                            tab.work = None;
                                            None
                                        } else {
                                            tab.work = None;
                                            tab.contents = Contents::Unavailable(error.to_string());
                                            tab.rows.clear();
                                            tab.backing =
                                                SelectionBacking::from_items(std::iter::empty());
                                            Some(Err(()))
                                        }
                                    }
                                }
                            };
                            match finished {
                                None => {}
                                Some(Ok(loc)) => {
                                    if need_save {
                                        browser.save_places();
                                    }
                                    browser.apply_location_view_settings(id, &loc);
                                    browser.reproject(id);
                                    browser.start_watch(id);
                                    browser.enrich_directory(id);
                                    browser.restore_active_scroll();
                                }
                                Some(Err(())) => {
                                    browser.apply_location_view_settings(id, request.location());
                                    browser.changed();
                                    browser.update_selection(|_| {});
                                }
                            }
                            break;
                        }
                    }
                }
            }));
        });
    }

    pub(super) fn finish_collection_load(&self, id: u64, request: &LoadRequest, loaded: Loaded) {
        let mut need_save = false;
        let loc_opt = {
            let mut state = self.0.state.borrow_mut();
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            let prior_pin = tab.pinned.then(|| tab.navigation.location().clone());
            match tab.navigation.finish_load(request, Ok(None)) {
                Ok(LoadOutcome::Stale) | Err(_) => return,
                Ok(LoadOutcome::Applied) => {}
            }
            tab.work = None;
            tab.enrichment = None;
            let prior_mounts = match &tab.contents {
                Contents::Network { mounts, .. } => mounts.clone(),
                _ => Vec::new(),
            };
            tab.contents = match loaded {
                Loaded::Unavailable(message) => Contents::Unavailable(message),
                Loaded::Places(places) => Contents::Places(places),
                Loaded::Network(Ok(mounts)) => Contents::Network {
                    mounts,
                    error: None,
                },
                Loaded::Network(Err(error)) => Contents::Network {
                    mounts: prior_mounts,
                    error: Some(error),
                },
                Loaded::Trash(entries) => Contents::Trash(entries),
                Loaded::Tags {
                    listing,
                    backend,
                    entries,
                } => Contents::Tags {
                    listing,
                    backend,
                    entries,
                },
                Loaded::Volumes => Contents::Volumes,
            };
            let loc = tab.navigation.location().clone();
            if request.records_recent() {
                state.places.record(loc.clone());
                need_save = true;
            }
            if let Some(old_loc) = prior_pin
                && sync_pinned_navigation(&mut state, &old_loc, &loc)
            {
                need_save = true;
            }
            loc
        };
        if need_save {
            self.save_places();
        }
        self.apply_location_view_settings(id, &loc_opt);
        self.reproject(id);
        self.restore_active_scroll();
    }

    fn start_watch(&self, id: u64) {
        let key = (Rc::as_ptr(&self.0) as usize, id);
        let watch_prefs = self.0.preferences.snapshot().files.watching;
        let path = self.read_untracked(|state| {
            let tab = state.tabs.iter().find(|tab| tab.id == id)?;
            if !matches!(tab.contents, Contents::Directory) {
                return None;
            }
            tab.navigation.path().map(Path::to_path_buf)
        });
        let Some(path) = path else {
            self.clear_watch(id);
            return;
        };

        ensure_watch_cleanup(self);
        let owner = self.0.tasks.named_child("Watch directory");
        let scope = (*owner).clone();
        let cancel = owner.token();
        WATCHES.with(|slot| {
            slot.borrow_mut().insert(key, owner);
        });

        let (tx, mut rx) = mpsc::unbounded_channel();
        let debounce = Duration::from_millis(watch_prefs.debounce_ms);
        let incremental_limit = watch_prefs.incremental_limit;
        drop(scope.spawn_named("Directory watcher", async move {
            let Ok(mut watching) = directory::Watching::with_limit(&path, incremental_limit) else {
                return;
            };
            loop {
                tokio::select! {
                    () = cancel.cancelled() => return,
                    alive = watching.changed() => {
                        if !alive {
                            return;
                        }
                    }
                }
                if !debounce.is_zero() {
                    let deadline = tokio::time::Instant::now() + debounce;
                    loop {
                        tokio::select! {
                            () = cancel.cancelled() => return,
                            () = tokio::time::sleep_until(deadline) => break,
                            alive = watching.changed() => {
                                if !alive {
                                    return;
                                }
                            }
                        }
                    }
                }
                let changes = watching.take_changes();
                if tx.send(changes).is_err() {
                    return;
                }
            }
        }));

        let weak = self.downgrade();
        let blocking_scope = scope;
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                while let Some(changes) = rx.recv().await {
                    let Some(browser) = weak.upgrade() else {
                        break;
                    };
                    let target = browser.read_untracked(|state| {
                        let tab = state.tabs.iter().find(|tab| tab.id == id)?;
                        if !matches!(tab.contents, Contents::Directory) {
                            return None;
                        }
                        let snapshot = tab.navigation.directory_arc()?;
                        let revision = tab.navigation.revision();
                        Some((revision, snapshot))
                    });
                    let Some((revision, snapshot)) = target else {
                        continue;
                    };
                    let cancel = blocking_scope.token();
                    let result = blocking_scope
                        .spawn_blocking_named("Reconcile directory changes", move || {
                            changes.apply_shared(&snapshot, &cancel)
                        })
                        .await;
                    let Some(browser) = weak.upgrade() else {
                        break;
                    };
                    match result {
                        Ok(Ok(Some(updated))) => {
                            let applied = browser.update(|state| {
                                let Some(tab) = state.tab_by_id(id) else {
                                    return false;
                                };
                                tab.navigation.update_directory(revision, updated)
                            });
                            if applied {
                                browser.reproject(id);
                                browser.enrich_directory(id);
                            }
                        }
                        Ok(Ok(None)) => {}
                        Ok(Err(error)) if error.is_cancelled() => {}
                        Ok(Err(_)) | Err(_) => {
                            browser.refresh_tab(id);
                        }
                    }
                }
            }));
        });
    }

    fn clear_watch(&self, id: u64) {
        let key = (Rc::as_ptr(&self.0) as usize, id);
        WATCHES.with(|slot| {
            slot.borrow_mut().remove(&key);
        });
        PLACE_ENTRIES.with(|slot| {
            slot.borrow_mut().remove(&key);
        });
    }
}

thread_local! {
    static WATCHES: RefCell<HashMap<(usize, u64), TaskOwner>> = RefCell::new(HashMap::new());
    static PLACE_ENTRIES: RefCell<HashMap<(usize, u64), HashMap<PathBuf, directory::Entry>>> =
        RefCell::new(HashMap::new());
    static WATCH_CLEANUP_WINDOWS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

fn ensure_watch_cleanup(browser: &Browser) {
    let window_key = Rc::as_ptr(&browser.0) as usize;
    let needs_cleanup = WATCH_CLEANUP_WINDOWS.with(|slot| slot.borrow_mut().insert(window_key));
    if needs_cleanup {
        browser.0.owner.with(|| {
            on_cleanup_local(move || {
                WATCHES.with(|slot| {
                    slot.borrow_mut().retain(|(win, _), _| *win != window_key);
                });
                PLACE_ENTRIES.with(|slot| {
                    slot.borrow_mut().retain(|(win, _), _| *win != window_key);
                });
                WATCH_CLEANUP_WINDOWS.with(|slot| {
                    slot.borrow_mut().remove(&window_key);
                });
            });
        });
    }
}

pub(super) fn pinned_tab_locations(tabs: &[Tab]) -> Vec<Location> {
    tabs.iter()
        .filter(|tab| tab.pinned)
        .map(|tab| tab.navigation.location().clone())
        .collect()
}

pub(super) fn sync_pinned_navigation(
    state: &mut State,
    old_loc: &Location,
    new_loc: &Location,
) -> bool {
    if old_loc == new_loc {
        return false;
    }
    let still_pinned_old = state
        .tabs
        .iter()
        .any(|tab| tab.pinned && tab.navigation.location() == old_loc);
    if !still_pinned_old {
        state.places.set_pinned(old_loc, false);
    }
    state.places.set_pinned(new_loc, true);
    let order = pinned_tab_locations(&state.tabs);
    state.places.reorder_pinned(order);
    true
}

fn apply_toggle_pin(
    tabs: &mut Vec<Tab>,
    active: &mut usize,
    index: usize,
) -> Option<(Location, bool)> {
    if index >= tabs.len() {
        return None;
    }
    let active_id = tabs[*active].id;
    let now_pinned = !tabs[index].pinned;
    tabs[index].pinned = now_pinned;
    let loc = tabs[index].navigation.location().clone();
    let tab = tabs.remove(index);
    let dest = tabs.iter().filter(|item| item.pinned).count();
    tabs.insert(dest, tab);
    if let Some(next_active) = tabs.iter().position(|item| item.id == active_id) {
        *active = next_active;
    }
    Some((loc, now_pinned))
}

fn apply_reorder_tab(
    tabs: &mut Vec<Tab>,
    active: &mut usize,
    from: usize,
    to: usize,
) -> Option<bool> {
    let len = tabs.len();
    if from >= len || to >= len || from == to {
        return None;
    }
    let pinned = tabs[from].pinned;
    let pinned_count = tabs.iter().filter(|tab| tab.pinned).count();
    let target = if pinned {
        to.min(pinned_count.saturating_sub(1))
    } else {
        to.max(pinned_count).min(len - 1)
    };
    if from == target {
        return None;
    }
    let active_id = tabs[*active].id;
    let tab = tabs.remove(from);
    tabs.insert(target, tab);
    if let Some(next_active) = tabs.iter().position(|item| item.id == active_id) {
        *active = next_active;
    }
    Some(pinned)
}

fn selected_place_for_favorite(tab: &Tab) -> Option<(RowKey, Location)> {
    if !matches!(
        tab.navigation.location(),
        Location::Recent | Location::Favorites | Location::Tags(_)
    ) {
        return None;
    }
    tab.selected().into_iter().find_map(|row| {
        let key = row.key();
        let loc = match row {
            Row::Place(loc) => Some(loc),
            Row::File(entry) => Some(Location::from(entry.path.as_path())),
            Row::Tag(tag) => scheme::TagAddress::new(&tag).ok().map(Location::Tags),
            _ => None,
        }?;
        Some((key, loc))
    })
}

const fn location_supports_grid(location: &Location) -> bool {
    !matches!(
        location,
        Location::Trash | Location::Volumes | Location::Network
    )
}

fn toggle_tab_or_selected_favorite(state: &mut State) -> (bool, bool) {
    let tab_loc = state.tab().navigation.location().clone();
    if matches!(
        tab_loc,
        Location::Recent | Location::Favorites | Location::Tags(_)
    ) {
        let Some((row_key, target_loc)) = selected_place_for_favorite(state.tab()) else {
            return (false, false);
        };
        state.places.toggle_favorite(target_loc.clone());
        let is_favorites_page = matches!(tab_loc, Location::Favorites);
        if is_favorites_page && !state.places.is_favorite(&target_loc) {
            state
                .tab_mut()
                .navigation
                .view_mut()
                .selection
                .remove(&row_key);
            return (true, true);
        }
        return (true, false);
    }
    if matches!(
        tab_loc,
        Location::Trash | Location::Volumes | Location::Network
    ) {
        return (false, false);
    }
    let current_loc = state
        .tab()
        .navigation
        .path()
        .map(Location::from)
        .unwrap_or(tab_loc);
    match current_loc {
        Location::Remote(address) => state.places.toggle_remote_favorite(address),
        other => state.places.toggle_favorite(other),
    }
    (true, false)
}

fn filter_remote_entries(
    entries: &[gds::service::remote::Entry],
    filter_lower: &str,
    show_hidden: bool,
) -> Vec<gds::service::remote::Entry> {
    entries
        .iter()
        .filter(|entry| {
            (show_hidden || !entry.name.starts_with('.'))
                && (filter_lower.is_empty() || entry.name.to_lowercase().contains(filter_lower))
        })
        .cloned()
        .collect()
}

fn remote_type_sort_rank(
    entry: &gds::service::remote::Entry,
) -> (u8, Option<directory::ContentCategory>) {
    match entry.kind {
        gds::service::remote::EntryKind::Directory => (0, None),
        gds::service::remote::EntryKind::File => (
            1,
            ContentType::from_filename(OsStr::new(&entry.name)).map(|ct| ct.category),
        ),
        gds::service::remote::EntryKind::SymbolicLink => (2, None),
        gds::service::remote::EntryKind::Other => (3, None),
    }
}

fn sort_remote_entries(entries: &mut [gds::service::remote::Entry], sort: sorting::Sort) {
    use std::cmp::Ordering;
    entries.sort_by(|left, right| {
        let dirs_first = (right.kind == gds::service::remote::EntryKind::Directory)
            .cmp(&(left.kind == gds::service::remote::EntryKind::Directory));
        if dirs_first != Ordering::Equal {
            return dirs_first;
        }
        let primary = match sort.key {
            sorting::Key::Size => left.size.cmp(&right.size),
            sorting::Key::Modified | sorting::Key::Accessed => left.modified.cmp(&right.modified),
            sorting::Key::Type | sorting::Key::DetailedType => {
                remote_type_sort_rank(left).cmp(&remote_type_sort_rank(right))
            }
            _ => Ordering::Equal,
        };
        let ordered = match sort.direction {
            sorting::Direction::Ascending => primary,
            sorting::Direction::Descending => primary.reverse(),
        };
        let tie = left.name.cmp(&right.name);
        if ordered == Ordering::Equal {
            match sort.direction {
                sorting::Direction::Ascending => tie,
                sorting::Direction::Descending => tie.reverse(),
            }
        } else {
            ordered.then(tie)
        }
    });
}

fn sortable_columns_for(state: &State) -> Vec<Column> {
    match &state.tab().contents {
        Contents::Directory => Column::ALL.to_vec(),
        Contents::Tags { entries, .. } if !entries.is_empty() => {
            vec![Column::Name, Column::Modified, Column::Type, Column::Size]
        }
        Contents::Remote { .. } => {
            vec![Column::Name, Column::Modified, Column::Type, Column::Size]
        }
        Contents::Trash(_)
        | Contents::Places(_)
        | Contents::Network { .. }
        | Contents::Tags { .. }
        | Contents::Volumes
        | Contents::LoadingCollection
        | Contents::Unavailable(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
