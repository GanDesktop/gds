//! Tab navigation, history traversal, directory/collection loading, watching, and tab lifecycle.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gds::service::task_manager::Owner as TaskOwner;
use gds::ui::IconName;
use tokio::sync::mpsc;
use zgui::prelude::*;
use zgui::reactive::{on_cleanup_local, spawn_local};

use super::super::{
    projection::rows::{
        self, Loaded, Rows, location_supports_grid, pinned_tab_locations, sync_pinned_navigation,
    },
    state::{
        context::Context,
        model::{
            AddressPresentation, ConfirmOptions, Contents, InitialVisit, Row, RowKey, State, Tab,
            remote_task_key,
        },
    },
};
use super::{enrichment::EnrichmentRunner, remote::RemoteLoader};
use crate::{
    entities::{
        file::{ViewMode, directory},
        location::{
            Location,
            navigation::{self, LoadOutcome, LoadRequest, PartialLoadOutcome},
            scheme, volumes,
        },
        tags,
    },
    features::{connect_server::Intent, file_operations::recycle},
    shared::{i18n, lib::selection::SelectionBacking},
};

fn fallback_network(ctx: &Context, id: u64) {
    Navigator(ctx).open_in_tab(id, Location::Network);
}

pub(in crate::pages::workspace) struct Navigator<'a>(pub &'a Context);

impl Navigator<'_> {
    pub(in crate::pages::workspace) fn open(&self, location: Location) {
        let id = self.0.read_untracked(|state| state.tab().id);
        self.open_in_tab(id, location);
    }

    pub(in crate::pages::workspace) fn open_in_tab(&self, id: u64, location: Location) {
        let rows = Rows(self.0);
        let in_picker = self.0.read_untracked(|state| state.picker.is_some());
        if in_picker && matches!(location, Location::Remote(_)) {
            self.0
                .report_notice(i18n::text_untracked("remote-picker-local"));
            rows.sync_address();
            return;
        }
        if self
            .0
            .is_background_task_busy_untracked(&remote_task_key(id))
            && self.0.read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .is_some_and(|tab| matches!(tab.contents, Contents::Remote { .. }))
            })
        {
            self.0
                .report_notice(i18n::text_untracked("remote-task-running"));
            rows.sync_address();
            return;
        }

        rows.save_active_scroll_if(id);
        if self
            .0
            .read_untracked(|state| state.tabs.get(state.active).is_some_and(|tab| tab.id == id))
        {
            self.0.reset_typeahead();
        }
        self.0
            .0
            .address_presentation
            .set(AddressPresentation::Breadcrumbs);
        let resolved = rows.resolve_path_untracked(&location);
        let show_hidden = self.0.0.preferences.snapshot().files.show_hidden;
        enum Dispatch {
            Remote(LoadRequest, gds::service::remote::Address),
            Standard(LoadRequest),
            UnavailableVolume { is_active: bool },
        }
        let dispatch = {
            let mut state = self.0.0.state.borrow_mut();
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
                    self.0.0.filter.set(String::new());
                }
                rows.sync_active_chrome();
                self.0.changed();
            }
            Dispatch::Remote(request, address) => {
                self.clear_watch(id);
                RemoteLoader(self.0).load_remote(id, request, address, fallback_network);
            }
            Dispatch::Standard(request) => self.start_load(id, request),
        }
    }

    pub(in crate::pages::workspace) fn open_address(&self) {
        let raw = self.0.0.address.get_untracked();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            Rows(self.0).sync_address();
            return;
        }
        let target = if scheme::recognizes(trimmed) {
            scheme::parse(trimmed).map_err(|error| error.to_string())
        } else {
            let (current_dir, home) = self.0.read_untracked(|state| {
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
                    .0
                    .address_presentation
                    .set(AddressPresentation::Breadcrumbs);
                self.open(location);
                self.0.focus_content();
            }
            Err(error) => {
                self.0.report_notice(error);
            }
        }
    }

    pub(in crate::pages::workspace) fn refresh(&self) {
        let id = self.0.read_untracked(|state| state.tab().id);
        self.refresh_tab(id);
    }

    pub(in crate::pages::workspace) fn reveal_paths(&self, paths: Vec<PathBuf>) {
        let active_id = self.0.read_untracked(|state| state.tab().id);
        self.0.update(|state| state.tab_mut().reveal = paths);
        Rows(self.0).reproject(active_id);
    }

    pub(in crate::pages::workspace) fn refresh_tab(&self, id: u64) {
        let rows = Rows(self.0);
        rows.save_active_scroll_if(id);
        let Some((location, in_picker)) = self.0.read_untracked(|state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| (tab.navigation.location().clone(), state.picker.is_some()))
        }) else {
            return;
        };
        if in_picker && matches!(location, Location::Remote(_)) {
            self.0
                .report_notice(i18n::text_untracked("remote-picker-local"));
            return;
        }
        let resolved = rows.resolve_path_untracked(&location);
        let cache = self.0.0.inspections.get_untracked();
        if let Some(dir) = resolved.as_deref() {
            cache.retry_failed_in(dir);
        } else {
            cache.retry_failed();
        }
        let pref_show_hidden = self.0.0.preferences.snapshot().files.show_hidden;
        enum Dispatch {
            Remote(LoadRequest, gds::service::remote::Address),
            Standard(LoadRequest),
            UnavailableVolume,
        }
        let dispatch = {
            let mut state = self.0.0.state.borrow_mut();
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
                    && !tab.navigation.has_directory()
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
                self.0.changed();
            }
            Dispatch::Remote(request, address) => {
                self.clear_watch(id);
                RemoteLoader(self.0).load_remote(id, request, address, fallback_network);
            }
            Dispatch::Standard(request) => self.start_load(id, request),
        }
    }

    pub(in crate::pages::workspace) fn refresh_tag_tabs(&self) {
        let ids: Vec<u64> = self.0.read_untracked(|state| {
            state
                .tabs
                .iter()
                .filter(|tab| matches!(tab.navigation.location(), Location::Tags(_)))
                .map(|tab| tab.id)
                .collect()
        });
        for id in ids {
            self.refresh_tab(id);
        }
    }

    pub(in crate::pages::workspace) fn refresh_location(&self, location: &Location) {
        let ids: Vec<u64> = self.0.read_untracked(|state| {
            state
                .tabs
                .iter()
                .filter(|tab| tab.navigation.location() == location)
                .map(|tab| tab.id)
                .collect()
        });
        for id in ids {
            self.refresh_tab(id);
        }
    }

    pub(in crate::pages::workspace) fn history(&self, forward: bool) {
        let target = self.0.read_untracked(|state| {
            let tab = state.tab();
            let (target_index, location) = if forward {
                tab.navigation.forward_target()
            } else {
                tab.navigation.back_target()
            }?;
            Some((tab.id, target_index, location.clone()))
        });
        if let Some((id, index, location)) = target {
            self.visit_history(id, index, &location);
        }
    }

    pub(in crate::pages::workspace) fn visit_history(
        &self,
        id: u64,
        index: usize,
        location: &Location,
    ) {
        let rows = Rows(self.0);
        let in_picker = self.0.read_untracked(|state| state.picker.is_some());
        if in_picker && matches!(location, Location::Remote(_)) {
            self.0
                .report_notice(i18n::text_untracked("remote-picker-local"));
            return;
        }
        if self
            .0
            .is_background_task_busy_untracked(&remote_task_key(id))
            && self.0.read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .is_some_and(|tab| matches!(tab.contents, Contents::Remote { .. }))
            })
        {
            self.0
                .report_notice(i18n::text_untracked("remote-task-running"));
            return;
        }

        rows.save_active_scroll_if(id);
        if self
            .0
            .read_untracked(|state| state.tabs.get(state.active).is_some_and(|tab| tab.id == id))
        {
            self.0.reset_typeahead();
        }
        self.0
            .0
            .address_presentation
            .set(AddressPresentation::Breadcrumbs);
        let resolved = rows.resolve_path_untracked(location);
        let pref_show_hidden = self.0.0.preferences.snapshot().files.show_hidden;
        enum Dispatch {
            Remote(LoadRequest, gds::service::remote::Address),
            Standard(LoadRequest),
            UnavailableVolume,
            Noop,
        }
        let dispatch = {
            let mut state = self.0.0.state.borrow_mut();
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
                rows.sync_active_chrome();
                self.0.changed();
            }
            Dispatch::Remote(request, address) => {
                self.clear_watch(id);
                RemoteLoader(self.0).load_remote(id, request, address, fallback_network);
            }
            Dispatch::Standard(request) => self.start_load(id, request),
        }
    }

    pub(in crate::pages::workspace) fn cancel_location_load(&self) {
        let (tab_id, unopened_remote) = self.0.update(|state| {
            let tab = state.tab_mut();
            tab.navigation.cancel_pending();
            tab.work = None;
            let unopened_remote = matches!(tab.navigation.location(), Location::Remote(_))
                && !matches!(tab.contents, Contents::Remote { .. });
            (tab.id, unopened_remote)
        });
        if self
            .0
            .0
            .connector
            .pending_untracked()
            .is_some_and(|(_, intent)| intent == Intent::Open)
        {
            self.0.0.connector.cancel();
        }
        if unopened_remote {
            self.open_in_tab(tab_id, Location::Network);
        } else {
            Rows(self.0).sync_address();
            self.0.changed();
        }
    }

    pub(in crate::pages::workspace) fn parent(&self) {
        let parent = self
            .0
            .read_untracked(|state| scheme::parent(state.tab().navigation.location()));
        if let Some(location) = parent {
            self.open(location);
        }
    }

    pub(in crate::pages::workspace) fn new_tab(&self, location: Option<Location>) {
        let in_picker = self.0.read_untracked(|state| state.picker.is_some());
        if in_picker && matches!(location, Some(Location::Remote(_))) {
            self.0
                .report_notice(i18n::text_untracked("remote-picker-local"));
            return;
        }
        let rows = Rows(self.0);
        rows.save_active_scroll();
        let new_id = {
            let mut state = self.0.0.state.borrow_mut();
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
        rows.sync_active_chrome();
        self.0.changed();
        self.refresh_tab(new_id);
        self.0.focus_content();
    }

    pub(in crate::pages::workspace) fn select_tab(&self, index: usize) {
        let rows = Rows(self.0);
        rows.save_active_scroll();
        let (changed, needs_hidden_refresh) = self.0.update(|state| {
            if index < state.tabs.len() && state.active != index {
                state.active = index;
                let tab = state.tab();
                let pref_hidden = self.0.0.preferences.snapshot().files.show_hidden;
                let mismatch = tab
                    .navigation
                    .directory_hidden_mismatch(pref_hidden || tab.reveal_hidden);
                (true, mismatch.then_some(tab.id))
            } else {
                (false, None)
            }
        });
        if changed {
            self.0
                .0
                .address_presentation
                .set(AddressPresentation::Breadcrumbs);
            rows.sync_active_chrome();
            self.0.update_selection(|_| {});
            rows.restore_active_scroll();
            if let Some(id) = needs_hidden_refresh {
                self.refresh_tab(id);
            }
        }
    }

    pub(in crate::pages::workspace) fn tab_index(&self, id: u64) -> Option<usize> {
        self.0
            .read_untracked(|state| state.tabs.iter().position(|tab| tab.id == id))
    }

    pub(in crate::pages::workspace) fn close_tab(&self, index: usize) {
        let rows = Rows(self.0);
        let target = self.0.read_untracked(|state| {
            let tab = state.tabs.get(index)?;
            let loc = tab.navigation.location();
            let title = match loc {
                Location::Volume { identity, relative } if relative.is_empty() => rows
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
            let ctx = self.0.clone();
            self.0
                .confirm_with_options(confirm_title, message, primary, options, move || {
                    Navigator(&ctx).close_tab_approved(id);
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
            let mut state = self.0.0.state.borrow_mut();
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
        let _ = self
            .0
            .finish_background_task(&remote_task_key(outcome.removed_id));
        if outcome.need_save_pins {
            self.0.save_places();
        }
        self.0
            .0
            .address_presentation
            .set(AddressPresentation::Breadcrumbs);
        let rows = Rows(self.0);
        rows.sync_active_chrome();
        self.0.update_selection(|_| {});
        self.0.changed();
        if let Some(home_id) = outcome.reopened_home_id {
            self.refresh_tab(home_id);
        } else {
            rows.restore_active_scroll();
        }
        self.0.focus_content();
    }

    pub(in crate::pages::workspace) fn toggle_pin(&self, index: usize) {
        let changed = self.0.update(|state| {
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
            self.0.save_places();
            self.0.changed();
        }
    }

    pub(in crate::pages::workspace) fn reorder_tab(&self, from: usize, to: usize) {
        let (changed, moved_pinned) = self.0.update(|state| {
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
                self.0.save_places();
            }
            self.0.changed();
        }
    }

    pub(in crate::pages::workspace) fn filter(&self, value: String) {
        self.0.reset_typeahead();
        let id = {
            let mut state = self.0.0.state.borrow_mut();
            let tab = state.tab_mut();
            if tab.navigation.view().filter == value {
                return;
            }
            tab.navigation.view_mut().filter = value.clone();
            tab.id
        };
        self.0.0.filter.set(value);
        Rows(self.0).reproject(id);
        EnrichmentRunner(self.0).enrich_directory(id);
    }

    pub(in crate::pages::workspace) fn toggle_view(&self) {
        let Some(id) = ({
            let mut state = self.0.0.state.borrow_mut();
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
        self.0.save_places();
        self.0.changed();
        EnrichmentRunner(self.0).enrich_directory(id);
    }

    pub(in crate::pages::workspace) fn favorite(&self) {
        let (mutated, selection_cleared) = {
            let mut state = self.0.0.state.borrow_mut();
            toggle_tab_or_selected_favorite(&mut state)
        };
        if !mutated {
            return;
        }
        self.0.save_places();
        self.refresh_favorites_tabs();
        if selection_cleared {
            self.0.update_selection(|_| {});
        }
        self.0.changed();
    }

    pub(in crate::pages::workspace) fn refresh_favorites_tabs(&self) {
        let ids: Vec<u64> = self.0.read_untracked(|state| {
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
        let rows = Rows(self.0);
        let local_paths: Vec<PathBuf> = locations
            .iter()
            .filter_map(|loc| rows.resolve_path_untracked(loc))
            .collect();
        if local_paths.is_empty() {
            rows::clear_place_entries(self.0, id);
            return;
        }
        let work = self.0.0.tasks.named_child("Inspect place metadata");
        let scope = (*work).clone();
        let cancel = work.token();
        {
            let mut state = self.0.0.state.borrow_mut();
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
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let Some(entries) = rx.recv().await else {
                    return;
                };
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                let still_places = ctx.read_untracked(|state| {
                    state
                        .tabs
                        .iter()
                        .find(|tab| tab.id == id)
                        .is_some_and(|tab| matches!(tab.contents, Contents::Places(_)))
                });
                if !still_places {
                    return;
                }
                rows::set_place_entries(&ctx, id, entries);
                ctx.update(|state| {
                    if let Some(tab) = state.tab_by_id(id) {
                        tab.work = None;
                    }
                });
                ctx.changed();
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
                const RECENT_DISPLAY_LIMIT: usize = 200;
                let work = self.0.0.tasks.named_child("Load recent files");
                let scope = (*work).clone();
                let cancel = work.token();
                {
                    let mut state = self.0.0.state.borrow_mut();
                    if let Some(tab) = state.tab_by_id(id) {
                        if !matches!(tab.contents, Contents::Recent(_)) {
                            tab.contents = Contents::LoadingCollection;
                            tab.rows.clear();
                        }
                        tab.work = Some(work);
                    }
                }
                self.0.changed();
                let (tx, mut rx) = mpsc::unbounded_channel();
                drop(
                    scope.spawn_blocking_named("Read recently-used.xbel", move || {
                        let outcome = gds::service::desktop::recent_files::list_default()
                            .map_err(|err| err.to_string())
                            .map(|items| {
                                let mut entries =
                                    Vec::with_capacity(items.len().min(RECENT_DISPLAY_LIMIT));
                                for item in items {
                                    if cancel.is_cancelled()
                                        || entries.len() >= RECENT_DISPLAY_LIMIT
                                    {
                                        break;
                                    }
                                    if let Ok(entry) = directory::Entry::inspect(&item.path) {
                                        entries.push(entry);
                                    }
                                }
                                entries
                            });
                        let _ = tx.send(outcome);
                    }),
                );
                let weak = self.0.downgrade();
                self.0.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let Some(result) = rx.recv().await else {
                            return;
                        };
                        let Some(ctx) = weak.upgrade() else {
                            return;
                        };
                        let loaded = match result {
                            Ok(entries) => Loaded::Recent(entries),
                            Err(error) => Loaded::Unavailable(error),
                        };
                        Rows(&ctx).finish_collection_load(id, &request, loaded);
                        Navigator(&ctx).start_recent_watch(id);
                    }));
                });
            }
            Location::Favorites => {
                let places = self
                    .0
                    .read_untracked(|state| state.places.favorites.clone());
                Rows(self.0).finish_collection_load(id, &request, Loaded::Places(places.clone()));
                self.inspect_places_metadata(id, &places);
            }
            Location::Volumes => {
                self.0.0.volumes.refresh();
                Rows(self.0).finish_collection_load(id, &request, Loaded::Volumes);
            }
            Location::Network => {
                let work = self.0.0.tasks.named_child("Load network mounts");
                let scope = (*work).clone();
                let is_active = self.0.read_untracked(|state| state.tab().id == id);
                {
                    let mut state = self.0.0.state.borrow_mut();
                    if let Some(tab) = state.tab_by_id(id) {
                        if !matches!(tab.contents, Contents::Network { .. }) {
                            tab.contents = Contents::LoadingCollection;
                            tab.rows.clear();
                        }
                        tab.work = Some(work);
                    }
                }
                if is_active {
                    Rows(self.0).sync_address();
                }
                self.0.changed();
                let (tx, mut rx) = mpsc::unbounded_channel();
                drop(scope.spawn_blocking_named("Read network mounts", move || {
                    let _ = tx.send(volumes::read_network_mounts());
                }));
                let weak = self.0.downgrade();
                self.0.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let Some(result) = rx.recv().await else {
                            return;
                        };
                        let Some(ctx) = weak.upgrade() else {
                            return;
                        };
                        let loaded = Loaded::Network(result.map_err(|error| error.to_string()));
                        Rows(&ctx).finish_collection_load(id, &request, loaded);
                    }));
                });
            }
            Location::Trash => {
                let work = self.0.0.tasks.named_child("Load Trash");
                let scope = (*work).clone();
                {
                    let mut state = self.0.0.state.borrow_mut();
                    if let Some(tab) = state.tab_by_id(id) {
                        if !matches!(tab.contents, Contents::Trash(_)) {
                            tab.contents = Contents::LoadingCollection;
                            tab.rows.clear();
                        }
                        tab.work = Some(work);
                    }
                }
                self.0.changed();
                let (tx, mut rx) = mpsc::unbounded_channel();
                drop(scope.spawn_blocking_named("List Trash", move || {
                    let _ = tx.send(recycle::list());
                }));
                let weak = self.0.downgrade();
                self.0.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let Some(result) = rx.recv().await else {
                            return;
                        };
                        let Some(ctx) = weak.upgrade() else {
                            return;
                        };
                        let loaded = match result {
                            Ok(entries) => Loaded::Trash(entries),
                            Err(error) => Loaded::Unavailable(error.to_string()),
                        };
                        Rows(&ctx).finish_collection_load(id, &request, loaded);
                        Navigator(&ctx).start_trash_watch(id);
                    }));
                });
            }
            Location::Tags(tag_addr) => {
                let tag_prefs = self.0.0.preferences.snapshot().tags;
                if !tag_prefs.enabled {
                    Rows(self.0).finish_collection_load(
                        id,
                        &request,
                        Loaded::Unavailable(i18n::text_untracked("tags-unavailable")),
                    );
                    return;
                }
                let work = self.0.0.tasks.named_child("Load tags");
                let scope = (*work).clone();
                let cancel = work.token();
                {
                    let mut state = self.0.0.state.borrow_mut();
                    if let Some(tab) = state.tab_by_id(id) {
                        if !matches!(tab.contents, Contents::Tags { .. }) {
                            tab.contents = Contents::LoadingCollection;
                            tab.rows.clear();
                        }
                        tab.work = Some(work);
                    }
                }
                self.0.changed();
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
                let weak = self.0.downgrade();
                self.0.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let Some(result) = rx.recv().await else {
                            return;
                        };
                        let Some(ctx) = weak.upgrade() else {
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
                        Rows(&ctx).finish_collection_load(id, &request, loaded);
                    }));
                });
            }
            Location::Directory(_) | Location::Volume { .. } => {
                Rows(self.0).finish_collection_load(
                    id,
                    &request,
                    Loaded::Unavailable(i18n::text_untracked("device-not-mounted")),
                );
            }
            Location::Remote(address) => {
                RemoteLoader(self.0).load_remote(id, request, address, fallback_network);
            }
        }
    }

    fn start_directory_load(&self, id: u64, request: LoadRequest) {
        let stream_options = self.0.0.preferences.snapshot().files.streaming.options();
        let (scope, cancel) = {
            let mut state = self.0.0.state.borrow_mut();
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            let work = self.0.0.tasks.named_child("Load directory");
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

        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                while let Some(msg) = rx.recv().await {
                    let Some(ctx) = weak.upgrade() else {
                        break;
                    };
                    match msg {
                        DirLoadMsg::Partial(snapshot) => {
                            let mut pin_saved = false;
                            let outcome = {
                                let mut state = ctx.0.state.borrow_mut();
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
                                ctx.save_places();
                            }
                            let rows = Rows(&ctx);
                            match outcome {
                                PartialLoadOutcome::FirstCommit => {
                                    rows.apply_location_view_settings(id, request.location());
                                    rows.reproject(id);
                                }
                                PartialLoadOutcome::Updated => {
                                    rows.reproject(id);
                                }
                                PartialLoadOutcome::Stale => break,
                            }
                        }
                        DirLoadMsg::Done(result) => {
                            let mut need_save = false;
                            let finished = {
                                let mut state = ctx.0.state.borrow_mut();
                                let Some(tab) = state.tab_by_id(id) else {
                                    break;
                                };
                                let prior_pin =
                                    tab.pinned.then(|| tab.navigation.location().clone());
                                let finish = tab.navigation.finish_load(&request, result);
                                match finish {
                                    Ok(LoadOutcome::Stale) => None,
                                    Ok(LoadOutcome::Applied) => {
                                        for warning in tab.navigation.directory_warnings() {
                                            tracing::warn!(
                                                path = %warning.path.display(),
                                                operation = ?warning.operation,
                                                error = %warning.source,
                                                "directory scan entry warning"
                                            );
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
                            let rows = Rows(&ctx);
                            match finished {
                                None => {}
                                Some(Ok(loc)) => {
                                    if need_save {
                                        ctx.save_places();
                                    }
                                    rows.apply_location_view_settings(id, &loc);
                                    rows.reproject(id);
                                    Navigator(&ctx).start_watch(id);
                                    EnrichmentRunner(&ctx).enrich_directory(id);
                                    rows.restore_active_scroll();
                                }
                                Some(Err(())) => {
                                    rows.apply_location_view_settings(id, request.location());
                                    ctx.changed();
                                    ctx.update_selection(|_| {});
                                }
                            }
                            break;
                        }
                    }
                }
            }));
        });
    }

    fn start_watch(&self, id: u64) {
        let key = (Rc::as_ptr(&self.0.0) as usize, id);
        let watch_prefs = self.0.0.preferences.snapshot().files.watching;
        let path = self.0.read_untracked(|state| {
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

        ensure_watch_cleanup(self.0);
        let owner = self.0.0.tasks.named_child("Watch directory");
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

        let weak = self.0.downgrade();
        let blocking_scope = scope;
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                while let Some(changes) = rx.recv().await {
                    let Some(ctx) = weak.upgrade() else {
                        break;
                    };
                    let target = ctx.read_untracked(|state| {
                        let tab = state.tabs.iter().find(|tab| tab.id == id)?;
                        if !matches!(tab.contents, Contents::Directory) {
                            return None;
                        }
                        tab.navigation.directory_revision_snapshot()
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
                    let Some(ctx) = weak.upgrade() else {
                        break;
                    };
                    match result {
                        Ok(Ok(Some(updated))) => {
                            let applied = ctx.update(|state| {
                                let Some(tab) = state.tab_by_id(id) else {
                                    return false;
                                };
                                tab.navigation.update_directory(revision, updated)
                            });
                            if applied {
                                Rows(&ctx).reproject(id);
                                EnrichmentRunner(&ctx).enrich_directory(id);
                            }
                        }
                        Ok(Ok(None)) => {}
                        Ok(Err(error)) if error.is_cancelled() => {}
                        Ok(Err(_)) | Err(_) => {
                            Navigator(&ctx).refresh_tab(id);
                        }
                    }
                }
            }));
        });
    }

    fn clear_watch(&self, id: u64) {
        let key = (Rc::as_ptr(&self.0.0) as usize, id);
        WATCHES.with(|slot| {
            slot.borrow_mut().remove(&key);
        });
        rows::clear_place_entries(self.0, id);
    }

    fn start_recent_watch(&self, id: u64) {
        let Some(xbel_path) = gds::service::desktop::recent_files::default_path() else {
            return;
        };
        let is_recent = self.0.read_untracked(|state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .is_some_and(|tab| matches!(tab.navigation.location(), Location::Recent))
        });
        if !is_recent {
            return;
        }

        ensure_watch_cleanup(self.0);
        let key = (Rc::as_ptr(&self.0.0) as usize, id);
        let owner = self.0.0.tasks.named_child("Watch recently-used.xbel");
        let scope = (*owner).clone();
        let cancel = owner.token();
        WATCHES.with(|slot| {
            slot.borrow_mut().insert(key, owner);
        });

        let (tx, mut rx) = mpsc::unbounded_channel();
        drop(scope.spawn_named("Recent files watcher", async move {
            let (raw_tx, mut raw_rx) = mpsc::channel(4);
            let mut sources = gds::service::desktop::watch::Sources::new(raw_tx);
            if sources.set_paths(vec![xbel_path]).is_err() {
                return;
            }
            loop {
                tokio::select! {
                    () = cancel.cancelled() => return,
                    event = raw_rx.recv() => {
                        if event.is_none() {
                            return;
                        }
                    }
                }
                let deadline = tokio::time::Instant::now() + Duration::from_millis(250);
                loop {
                    tokio::select! {
                        () = cancel.cancelled() => return,
                        () = tokio::time::sleep_until(deadline) => break,
                        event = raw_rx.recv() => {
                            if event.is_none() {
                                return;
                            }
                        }
                    }
                }
                let _ = sources.refresh();
                if tx.send(()).is_err() {
                    return;
                }
            }
        }));

        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                while rx.recv().await.is_some() {
                    let Some(ctx) = weak.upgrade() else {
                        break;
                    };
                    let still_recent = ctx.read_untracked(|state| {
                        state
                            .tabs
                            .iter()
                            .find(|tab| tab.id == id)
                            .is_some_and(|tab| {
                                matches!(tab.navigation.location(), Location::Recent)
                            })
                    });
                    if still_recent {
                        Navigator(&ctx).refresh_tab(id);
                    }
                }
            }));
        });
    }

    fn start_trash_watch(&self, id: u64) {
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            let _ = id;
        }
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            if !recycle::supported() {
                return;
            }
            let extra_dirs = self.0.read_untracked(|state| {
                let tab = state.tabs.iter().find(|tab| tab.id == id)?;
                if !matches!(tab.navigation.location(), Location::Trash)
                    || tab.navigation.is_loading()
                {
                    return None;
                }
                let dirs = match &tab.contents {
                    Contents::Trash(entries) => entries
                        .iter()
                        .filter_map(|entry| entry.info_directory().map(Path::to_path_buf))
                        .collect(),
                    _ => Vec::new(),
                };
                Some(dirs)
            });
            let Some(extra_dirs) = extra_dirs else {
                return;
            };

            ensure_watch_cleanup(self.0);
            let key = (Rc::as_ptr(&self.0.0) as usize, id);
            let owner = self.0.0.tasks.named_child("Watch Trash");
            let scope = (*owner).clone();
            let cancel = owner.token();
            WATCHES.with(|slot| {
                slot.borrow_mut().insert(key, owner);
            });

            let (tx, mut rx) = mpsc::unbounded_channel();
            let blocking_scope = scope.clone();
            drop(scope.spawn_named("Trash watcher", async move {
                let Ok(mut dirs) = blocking_scope
                    .spawn_blocking_named(
                        "Discover Trash watch directories",
                        recycle::watch_directories,
                    )
                    .await
                else {
                    return;
                };
                for dir in extra_dirs {
                    if !dirs.contains(&dir) {
                        dirs.push(dir);
                    }
                }
                if dirs.is_empty() || cancel.is_cancelled() {
                    return;
                }

                let (raw_tx, mut raw_rx) = mpsc::channel(4);
                let mut watch = recycle::TrashWatch::new(raw_tx);
                if let Err(error) = watch.set_directories(dirs) {
                    tracing::debug!(%error, "Unable to start Trash watcher");
                    return;
                }

                enum WaitEvent {
                    Cancelled,
                    Changed,
                    Closed,
                }
                enum DebounceEvent {
                    Cancelled,
                    Elapsed,
                    Changed,
                    Closed,
                }

                loop {
                    let wait = tokio::select! {
                        () = cancel.cancelled() => WaitEvent::Cancelled,
                        event = raw_rx.recv() => match event {
                            Some(()) => WaitEvent::Changed,
                            None => WaitEvent::Closed,
                        },
                    };
                    match wait {
                        WaitEvent::Cancelled | WaitEvent::Closed => return,
                        WaitEvent::Changed => {}
                    }

                    let deadline = tokio::time::Instant::now() + Duration::from_millis(250);
                    loop {
                        let event = tokio::select! {
                            () = cancel.cancelled() => DebounceEvent::Cancelled,
                            () = tokio::time::sleep_until(deadline) => DebounceEvent::Elapsed,
                            item = raw_rx.recv() => match item {
                                Some(()) => DebounceEvent::Changed,
                                None => DebounceEvent::Closed,
                            },
                        };
                        match event {
                            DebounceEvent::Cancelled | DebounceEvent::Closed => return,
                            DebounceEvent::Elapsed => break,
                            DebounceEvent::Changed => {}
                        }
                    }

                    let _ = watch.refresh();
                    if tx.send(()).is_err() {
                        return;
                    }
                }
            }));

            let weak = self.0.downgrade();
            self.0.0.owner.with(|| {
                drop(spawn_local(async move {
                    while rx.recv().await.is_some() {
                        let Some(ctx) = weak.upgrade() else {
                            break;
                        };
                        let still_trash = ctx.read_untracked(|state| {
                            state
                                .tabs
                                .iter()
                                .find(|tab| tab.id == id)
                                .is_some_and(|tab| {
                                    matches!(tab.navigation.location(), Location::Trash)
                                })
                        });
                        if still_trash {
                            Navigator(&ctx).refresh_tab(id);
                        } else {
                            break;
                        }
                    }
                }));
            });
        }
    }
}

thread_local! {
    static WATCHES: RefCell<HashMap<(usize, u64), TaskOwner>> = RefCell::new(HashMap::new());
    static WATCH_CLEANUP_WINDOWS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

fn ensure_watch_cleanup(ctx: &Context) {
    let window_key = Rc::as_ptr(&ctx.0) as usize;
    let needs_cleanup = WATCH_CLEANUP_WINDOWS.with(|slot| slot.borrow_mut().insert(window_key));
    if needs_cleanup {
        ctx.0.owner.with(|| {
            on_cleanup_local(move || {
                WATCHES.with(|slot| {
                    slot.borrow_mut().retain(|(win, _), _| *win != window_key);
                });
                WATCH_CLEANUP_WINDOWS.with(|slot| {
                    slot.borrow_mut().remove(&window_key);
                });
            });
        });
    }
}

pub(super) fn apply_toggle_pin(
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

pub(super) fn apply_reorder_tab(
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

pub(super) fn toggle_tab_or_selected_favorite(state: &mut State) -> (bool, bool) {
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
