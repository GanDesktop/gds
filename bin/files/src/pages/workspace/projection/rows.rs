//! Pure row projection, location/volume resolution, scroll, and sort/column configuration.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use zgui::geom::{DevicePx, Point};
use zgui::prelude::*;
use zgui::reactive::on_cleanup_local;
use zgui::view::{ScrollBehavior, ScrollTarget};

use super::super::state::{
    context::Context,
    menus::{ColumnScope, Surface},
    model::{Contents, Row, State, Tab},
};
use super::geometry::Geometry;
use crate::{
    entities::{
        file::{
            ViewMode,
            columns::Column,
            directory::{self, content_type_from_filename},
            sorting::{self, Sort},
        },
        location::{
            Location, VolumeIdentity,
            navigation::{LoadOutcome, LoadRequest},
            volumes,
        },
        tags,
    },
    features::file_operations::recycle,
    shared::lib::selection::SelectionBacking,
};

pub(in crate::pages::workspace) enum Loaded {
    Unavailable(String),
    Recent(Vec<directory::Entry>),
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

thread_local! {
    static PLACE_ENTRIES: RefCell<HashMap<(usize, u64), HashMap<PathBuf, directory::Entry>>> =
        RefCell::new(HashMap::new());
    static PLACE_CLEANUP_WINDOWS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

fn ensure_place_cleanup(ctx: &Context) {
    let window_key = Rc::as_ptr(&ctx.0) as usize;
    let needs_cleanup = PLACE_CLEANUP_WINDOWS.with(|slot| slot.borrow_mut().insert(window_key));
    if needs_cleanup {
        ctx.0.owner.with(|| {
            on_cleanup_local(move || {
                PLACE_ENTRIES.with(|slot| {
                    slot.borrow_mut().retain(|(win, _), _| *win != window_key);
                });
                PLACE_CLEANUP_WINDOWS.with(|slot| {
                    slot.borrow_mut().remove(&window_key);
                });
            });
        });
    }
}

pub(in crate::pages::workspace) fn set_place_entries(
    ctx: &Context,
    id: u64,
    entries: HashMap<PathBuf, directory::Entry>,
) {
    ensure_place_cleanup(ctx);
    let key = (Rc::as_ptr(&ctx.0) as usize, id);
    PLACE_ENTRIES.with(|slot| {
        slot.borrow_mut().insert(key, entries);
    });
}

pub(in crate::pages::workspace) fn clear_place_entries(ctx: &Context, id: u64) {
    let key = (Rc::as_ptr(&ctx.0) as usize, id);
    PLACE_ENTRIES.with(|slot| {
        slot.borrow_mut().remove(&key);
    });
}

pub(in crate::pages::workspace) struct Rows<'a>(pub &'a Context);

impl Rows<'_> {
    pub(in crate::pages::workspace) fn resolve_path(&self, location: &Location) -> Option<PathBuf> {
        self.resolve_path_with(location, |identity| self.volume_at_location(identity))
    }

    pub(in crate::pages::workspace) fn resolve_path_untracked(
        &self,
        location: &Location,
    ) -> Option<PathBuf> {
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
                        .0
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

    pub(in crate::pages::workspace) fn volume_at_location(
        &self,
        identity: &VolumeIdentity,
    ) -> Option<volumes::Volume> {
        Self::find_volume_in_snapshot(self.0.0.volumes.snapshot(), identity)
    }

    pub(in crate::pages::workspace) fn volume_at_location_untracked(
        &self,
        identity: &VolumeIdentity,
    ) -> Option<volumes::Volume> {
        Self::find_volume_in_snapshot(self.0.0.volumes.snapshot_untracked(), identity)
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

    pub(in crate::pages::workspace) fn volume_identity(volume: &volumes::Volume) -> VolumeIdentity {
        if let Some(stable) = &volume.id.stable {
            VolumeIdentity::Stable(stable.clone())
        } else {
            VolumeIdentity::Attachment(volume.id.attachment.clone())
        }
    }

    pub(in crate::pages::workspace) fn location_title(&self, location: &Location) -> String {
        match location {
            Location::Volume { identity, relative } if relative.is_empty() => self
                .volume_at_location_untracked(identity)
                .map_or_else(|| location.label(), |volume| volume.name),
            _ => location.label(),
        }
    }

    pub(in crate::pages::workspace) fn location_address(
        location: &Location,
        _path: Option<&Path>,
    ) -> String {
        location.display_address()
    }

    pub(in crate::pages::workspace) fn local_location(&self, path: &Path) -> Location {
        Location::from(path)
    }

    pub(in crate::pages::workspace) fn place_entry(
        &self,
        location: &Location,
    ) -> Option<directory::Entry> {
        let path = self.resolve_path(location)?;
        let tab_id = self.0.read(|state| {
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
                let key = (Rc::as_ptr(&self.0.0) as usize, id);
                PLACE_ENTRIES.with(|slot| {
                    slot.borrow()
                        .get(&key)
                        .and_then(|map| map.get(&path).cloned())
                })
            }
        }
    }

    pub(in crate::pages::workspace) fn sortable_columns(&self) -> Vec<Column> {
        self.0.read(sortable_columns_for)
    }

    pub(in crate::pages::workspace) fn sortable_columns_untracked(&self) -> Vec<Column> {
        self.0.read_untracked(sortable_columns_for)
    }

    pub(in crate::pages::workspace) fn configurable_columns(&self) -> bool {
        self.0.read(is_configurable_columns)
    }

    pub(in crate::pages::workspace) fn configurable_columns_untracked(&self) -> bool {
        self.0.read_untracked(is_configurable_columns)
    }

    pub(in crate::pages::workspace) fn reproject(&self, id: u64) {
        let volumes_snapshot = self.0.0.volumes.snapshot_untracked().volumes;
        let pref_show_hidden = self.0.0.preferences.snapshot().files.show_hidden;
        let reveal_scroll_idx = {
            let mut state = self.0.0.state.borrow_mut();
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
                    let Some(entries) = tab.navigation.directory_entries() else {
                        tab.rows.clear();
                        tab.backing = SelectionBacking::from_items(std::iter::empty());
                        return;
                    };
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
                    indices
                        .into_iter()
                        .map(|idx| Row::File(entries[idx].clone()))
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
                Contents::Recent(entries) => entries
                    .iter()
                    .filter(|entry| {
                        if let Some(projection) = picker_projection.as_ref()
                            && !projection.keeps(entry)
                        {
                            return false;
                        }
                        filter_lower.is_empty() || entry.matches_filter(&filter_lower)
                    })
                    .cloned()
                    .map(Row::File)
                    .collect(),
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

        self.0.changed();
        if self
            .0
            .read_untracked(|state| state.tabs.get(state.active).is_some_and(|tab| tab.id == id))
        {
            self.0.update_selection(|_| {});
            self.sync_address();
            if let Some(row_idx) = reveal_scroll_idx {
                self.scroll_row_into_view(row_idx);
            }
        }
    }

    pub(in crate::pages::workspace) fn sync_address(&self) {
        let address = self.0.read_untracked(|state| {
            let tab = state.tab();
            tab.navigation
                .pending_location()
                .unwrap_or_else(|| tab.navigation.location())
                .display_address()
        });
        self.0.0.address.set(address);
    }

    pub(in crate::pages::workspace) fn sync_active_chrome(&self) {
        self.0.reset_typeahead();
        let (address, filter, details, widths) = self.0.read_untracked(|state| {
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
        self.0.0.address.set(address);
        self.0.0.filter.set(filter);
        self.0.0.details.set(details);
        self.0.0.column_widths.set(widths);
    }

    pub(in crate::pages::workspace) fn apply_location_view_settings(
        &self,
        id: u64,
        location: &Location,
    ) {
        let is_active = {
            let mut state = self.0.0.state.borrow_mut();
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

    pub(in crate::pages::workspace) fn finish_collection_load(
        &self,
        id: u64,
        request: &LoadRequest,
        loaded: Loaded,
    ) {
        let mut need_save = false;
        let loc_opt = {
            let mut state = self.0.0.state.borrow_mut();
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
                Loaded::Recent(entries) => Contents::Recent(entries),
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
            self.0.save_places();
        }
        self.apply_location_view_settings(id, &loc_opt);
        self.reproject(id);
        self.restore_active_scroll();
    }

    pub(in crate::pages::workspace) fn scroll_row_into_view(&self, index: usize) {
        let geometry = Geometry(self.0);
        let row_h = geometry.row_height_untracked().max(1.0);
        let grid = geometry.grid_geometry_untracked();
        let scale = self.0.0.list.scale().max(1.0);
        let mode = self.0.read_untracked(|state| state.tab().mode);
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
        let pos = self.0.0.list.scroll_position();
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
            self.0.0.list.scroll_to(
                ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(top * scale))),
                ScrollBehavior::Instant,
            );
        }
    }

    pub(in crate::pages::workspace) fn save_active_scroll(&self) {
        let scale = self.0.0.list.scale().max(1.0);
        let top = (self.0.0.list.scroll_offset().y.0 / scale).max(0.0);
        self.0.update(|state| {
            state.tab_mut().navigation.view_mut().scroll = top;
        });
    }

    pub(in crate::pages::workspace) fn save_active_scroll_if(&self, id: u64) {
        let is_active = self.0.read_untracked(|state| state.tab().id == id);
        if is_active {
            self.save_active_scroll();
        }
    }

    pub(in crate::pages::workspace) fn restore_active_scroll(&self) {
        let top = self
            .0
            .read_untracked(|state| state.tab().navigation.view().scroll.max(0.0));
        let scale = self.0.0.list.scale().max(1.0);
        self.0.0.list.scroll_to(
            ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(top * scale))),
            ScrollBehavior::Instant,
        );
    }

    pub(in crate::pages::workspace) fn apply_tab_sort(
        &self,
        update_sort: impl FnOnce(Sort) -> Sort,
    ) -> u64 {
        self.0.reset_typeahead();
        let id = self.0.update(|state| {
            let tab = state.tab_mut();
            tab.sort = update_sort(tab.sort);
            let id = tab.id;
            let location = tab.navigation.location().clone();
            let sort = tab.sort;
            state.places.set_sort(location, sort);
            id
        });
        self.reproject(id);
        self.0.save_places();
        id
    }

    pub(in crate::pages::workspace) fn sort_by(&self, column: Column) -> u64 {
        self.apply_tab_sort(|current| current.next(column))
    }

    pub(in crate::pages::workspace) fn configure_columns(&self) {
        if !self.configurable_columns_untracked()
            || self.0.read_untracked(|state| {
                state.tab().mode == ViewMode::Grid || state.tab().navigation.is_loading()
            })
        {
            return;
        }
        let (location, layout, default_scope) = self.0.read_untracked(|state| {
            let loc = state.tab().navigation.location().clone();
            let cols = state.tab().columns.clone();
            let scope = if state.places.columns_for(&loc).is_some() {
                ColumnScope::Location
            } else {
                ColumnScope::Default
            };
            (loc, cols, scope)
        });
        let (layout_sig, scope_sig) = self.0.0.owner.with(|| {
            (
                RwSignal::new_local(layout),
                RwSignal::new_local(default_scope),
            )
        });
        self.0.0.menus.surface.set(Surface::Columns {
            location,
            layout: layout_sig,
            scope: scope_sig,
        });
    }
}

pub(in crate::pages::workspace) fn pinned_tab_locations(tabs: &[Tab]) -> Vec<Location> {
    tabs.iter()
        .filter(|tab| tab.pinned)
        .map(|tab| tab.navigation.location().clone())
        .collect()
}

pub(in crate::pages::workspace) fn sync_pinned_navigation(
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

pub(in crate::pages::workspace) const fn location_supports_grid(location: &Location) -> bool {
    !matches!(
        location,
        Location::Trash | Location::Volumes | Location::Network
    )
}

pub(in crate::pages::workspace) fn filter_remote_entries(
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
            content_type_from_filename(OsStr::new(&entry.name)).map(|ct| ct.category),
        ),
        gds::service::remote::EntryKind::SymbolicLink => (2, None),
        gds::service::remote::EntryKind::Other => (3, None),
    }
}

pub(in crate::pages::workspace) fn sort_remote_entries(
    entries: &mut [gds::service::remote::Entry],
    sort: sorting::Sort,
) {
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

pub(in crate::pages::workspace) fn sortable_columns_for(state: &State) -> Vec<Column> {
    match &state.tab().contents {
        Contents::Directory => Column::ALL.to_vec(),
        Contents::Tags { entries, .. } if !entries.is_empty() => {
            vec![Column::Name, Column::Modified, Column::Type, Column::Size]
        }
        Contents::Remote { .. } => {
            vec![Column::Name, Column::Modified, Column::Type, Column::Size]
        }
        Contents::Trash(_)
        | Contents::Recent(_)
        | Contents::Places(_)
        | Contents::Network { .. }
        | Contents::Tags { .. }
        | Contents::Volumes
        | Contents::LoadingCollection
        | Contents::Unavailable(_) => Vec::new(),
    }
}

fn is_configurable_columns(state: &State) -> bool {
    matches!(
        state.tab().contents,
        Contents::Directory | Contents::Tags { .. }
    ) && !state.tab().is_pure_tag_catalogue()
}
