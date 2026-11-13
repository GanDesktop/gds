pub(super) use crate::entities::location::{Collection, Location, LocationTarget, VolumeIdentity};
use gds::appearance::tokens;

mod chrome;
mod navigation;
mod surfaces;
mod workflows;

pub(in crate::pages::workspace) use chrome::*;
pub(in crate::pages::workspace) use navigation::*;
pub(in crate::pages::workspace) use surfaces::*;
pub(in crate::pages::workspace) use workflows::*;

use gds::service::task_manager::Scope;
use gds::service::task_manager::Task as IoTask;
pub(super) use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::entities::location::volumes as storage;
pub(super) use crate::entities::location::volumes::VolumesExt;
use crate::entities::preferences as configuration;
pub(super) use crate::entities::preferences::PreferencesExt;
pub(super) use crate::shared::lib::clipboard::ClipboardExt;
pub(super) use crate::shared::lib::selection::{Selection, SelectionBacking};
use crate::shared::ui::actions::*;
pub(super) use gds::app::AppExt;
use gds::{i18n, service::desktop, ui, ui::notifications};
pub(super) use gpui_kit::component::WindowExt;
use gpui_kit::component::input::{InputEvent, InputState};
pub(super) use gpui_kit::*;

use crate::entities::file::directory::{self, ReadOptions};
pub(super) use crate::entities::file::{ViewMode, sorting};
use crate::entities::location::navigation::{
    LoadOutcome, LoadRequest, Navigation, PartialLoadOutcome, resolve_input,
};
use crate::entities::location::places::Places;

pub struct NewWindowRequested(pub PathBuf);
impl EventEmitter<NewWindowRequested> for Browser {}

pub struct LaunchOptions {
    pub places: Result<Places, crate::entities::location::places::Error>,
    pub start: PathBuf,
    pub home: PathBuf,
    /// Present when the window is a file picker instead of a browser.
    pub picker: Option<crate::features::pick_files::Options>,
}

/// Toast slots that keep repeated reports of one origin from stacking.
pub(crate) struct LoadNotice;
pub(crate) struct OpenNotice;
pub(crate) struct WarningNotice;
pub(crate) struct SettingsNotice;
pub(crate) struct PlacesNotice;
pub(crate) struct ClipboardNotice;
pub(crate) struct TerminalNotice;
pub(crate) struct RenameNotice;
pub(crate) struct ColumnNotice;

#[derive(Default)]
enum CollectionView {
    #[default]
    None,
    Place(Location),
    Trash(recycle::ViewState),
    Volume(storage::VolumeId),
}

#[derive(Default)]
pub(super) struct ViewState {
    pub(super) selected: Selection<PathBuf>,
    scroll_offset: f32,
    filter: String,
    collection: CollectionView,
}

impl ViewState {
    fn place(&self) -> Option<&Location> {
        match &self.collection {
            CollectionView::Place(location) => Some(location),
            _ => None,
        }
    }

    fn set_place(&mut self, location: Option<Location>) {
        self.collection = match location {
            Some(location) => CollectionView::Place(location),
            None => CollectionView::None,
        };
    }

    fn trash(&self) -> Option<&recycle::ViewState> {
        match &self.collection {
            CollectionView::Trash(trash) => Some(trash),
            _ => None,
        }
    }

    fn set_trash(&mut self, trash: recycle::ViewState) {
        self.collection = CollectionView::Trash(trash);
    }

    fn volume(&self) -> Option<&storage::VolumeId> {
        match &self.collection {
            CollectionView::Volume(id) => Some(id),
            _ => None,
        }
    }

    fn set_volume(&mut self, volume: Option<storage::VolumeId>) {
        self.collection = match volume {
            Some(id) => CollectionView::Volume(id),
            None => CollectionView::None,
        };
    }
}

enum TabBackend {
    Local {
        watch: Option<watching::Watch>,
        device: Option<devices::DeviceLocation>,
        load: Option<enrichment::LocalJob>,
        enrich: Option<enrichment::EnrichJob>,
    },
    Tags {
        contents: Option<Result<tags::Contents, String>>,
        load: Option<Task<()>>,
    },
    Remote {
        directory: Option<remote::Directory>,
        load: Option<Task<()>>,
    },
}

impl Default for TabBackend {
    fn default() -> Self {
        Self::Local {
            watch: None,
            device: None,
            load: None,
            enrich: None,
        }
    }
}

struct SortedProjection {
    source: Arc<[directory::Entry]>,
    sort: sorting::Sort,
    entries: Arc<[directory::Entry]>,
}

pub(super) struct OpenTab {
    pub(super) id: u64,
    needs_refresh: bool,
    pub(super) projection_dirty: bool,
    pub(super) navigation: Navigation<ViewState>,
    backend: TabBackend,
    pub(super) table_rows: Arc<[directory::Entry]>,
    sorted_projection: Option<SortedProjection>,
    row_backing: Option<Arc<SelectionBacking<PathBuf>>>,
    scroll: UniformListScrollHandle,
    pub(super) view_mode: ViewMode,
    grid: grid::Geometry,
    pending_selection: Option<PathBuf>,
    pending_reveal: Option<Vec<PathBuf>>,
    reveal_hidden: bool,
    anchor: Option<PathBuf>,
    anchor_index: Option<usize>,
    cursor: Option<PathBuf>,
    cursor_index: Option<usize>,
    sort: sorting::Sort,
    pinned: bool,
    /// The picker's type-filter projection for this tab, if this window is a
    /// picker. It is applied together with the name filter so a directory read
    /// never replaces the narrowed list with the unfiltered one.
    picker: Option<picker::Projection>,
}

#[derive(Clone, Copy, Default)]
pub(in crate::pages::workspace) struct TabCycleTargets {
    pub(in crate::pages::workspace) previous: Option<u64>,
    pub(in crate::pages::workspace) next: Option<u64>,
}

/// Minimum inclusive span before materializing a full `SelectionBacking` for `O(1)` range slicing.
const RANGE_BACKING_MIN_SPAN: usize = 64;

impl OpenTab {
    fn new(id: u64, location: Location, path: Option<PathBuf>, pinned: bool) -> Self {
        Self {
            id,
            navigation: Navigation::new(location, path),
            needs_refresh: false,
            projection_dirty: false,
            backend: TabBackend::default(),
            table_rows: Arc::from([]),
            sorted_projection: None,
            row_backing: None,
            scroll: UniformListScrollHandle::new(),
            view_mode: ViewMode::default(),
            grid: grid::Geometry::new(1., 1.),
            pending_selection: None,
            pending_reveal: None,
            reveal_hidden: false,
            anchor: None,
            anchor_index: None,
            cursor: None,
            cursor_index: None,
            sort: sorting::Sort::default(),
            pinned,
            picker: None,
        }
    }

    fn path(&self) -> Option<&Path> {
        self.navigation.path()
    }
    fn location(&self) -> Location {
        self.navigation.location().clone()
    }
    fn is_loading(&self) -> bool {
        self.navigation.is_loading()
    }
    fn filter(&self) -> &str {
        &self.navigation.view().filter
    }
    fn set_filter(&mut self, filter: impl Into<String>) {
        self.navigation.view_mut().filter = filter.into();
    }
    fn clear_filter(&mut self) {
        self.navigation.view_mut().filter.clear();
    }
    fn watch(&self) -> Option<&watching::Watch> {
        match &self.backend {
            TabBackend::Local { watch, .. } => watch.as_ref(),
            _ => None,
        }
    }
    fn set_watch(&mut self, next: Option<watching::Watch>) {
        match &mut self.backend {
            TabBackend::Local { watch, .. } => *watch = next,
            _ if next.is_some() => {
                self.backend = TabBackend::Local {
                    watch: next,
                    device: None,
                    load: None,
                    enrich: None,
                };
            }
            _ => {}
        }
    }
    fn device(&self) -> Option<&devices::DeviceLocation> {
        match &self.backend {
            TabBackend::Local { device, .. } => device.as_ref(),
            _ => None,
        }
    }
    fn device_mut(&mut self) -> Option<&mut devices::DeviceLocation> {
        match &mut self.backend {
            TabBackend::Local { device, .. } => device.as_mut(),
            _ => None,
        }
    }
    fn set_device(&mut self, next: Option<devices::DeviceLocation>) {
        match &mut self.backend {
            TabBackend::Local { device, .. } => *device = next,
            _ => {
                self.backend = TabBackend::Local {
                    watch: None,
                    device: next,
                    load: None,
                    enrich: None,
                };
            }
        }
    }
    fn set_local_load(&mut self, next: Option<enrichment::LocalJob>) {
        match &mut self.backend {
            TabBackend::Local { load, .. } => *load = next,
            _ if next.is_some() => {
                self.backend = TabBackend::Local {
                    watch: None,
                    device: None,
                    load: next,
                    enrich: None,
                };
            }
            _ => {}
        }
    }
    pub(super) fn local_enrich(&self) -> Option<&enrichment::EnrichJob> {
        match &self.backend {
            TabBackend::Local { enrich, .. } => enrich.as_ref(),
            _ => None,
        }
    }
    pub(super) fn set_local_enrich(&mut self, next: Option<enrichment::EnrichJob>) {
        match &mut self.backend {
            TabBackend::Local { enrich, .. } => *enrich = next,
            _ if next.is_some() => {
                self.backend = TabBackend::Local {
                    watch: None,
                    device: None,
                    load: None,
                    enrich: next,
                };
            }
            _ => {}
        }
    }
    fn tags(&self) -> Option<&Result<tags::Contents, String>> {
        match &self.backend {
            TabBackend::Tags { contents, .. } => contents.as_ref(),
            _ => None,
        }
    }
    fn set_tags(&mut self, next: Option<Result<tags::Contents, String>>) {
        match &mut self.backend {
            TabBackend::Tags { contents, .. } => *contents = next,
            _ if next.is_some() => {
                self.backend = TabBackend::Tags {
                    contents: next,
                    load: None,
                };
            }
            _ => {}
        }
    }
    fn set_tags_load(&mut self, next: Option<Task<()>>) {
        match &mut self.backend {
            TabBackend::Tags { load, .. } => *load = next,
            _ if next.is_some() => {
                self.backend = TabBackend::Tags {
                    contents: None,
                    load: next,
                };
            }
            _ => {}
        }
    }
    fn remote(&self) -> Option<&remote::Directory> {
        match &self.backend {
            TabBackend::Remote { directory, .. } => directory.as_ref(),
            _ => None,
        }
    }
    fn set_remote(&mut self, next: Option<remote::Directory>) {
        match &mut self.backend {
            TabBackend::Remote { directory, .. } => *directory = next,
            _ if next.is_some() => {
                self.backend = TabBackend::Remote {
                    directory: next,
                    load: None,
                };
            }
            _ => {}
        }
    }
    fn set_remote_load(&mut self, next: Option<Task<()>>) {
        match &mut self.backend {
            TabBackend::Remote { load, .. } => *load = next,
            _ if next.is_some() => {
                self.backend = TabBackend::Remote {
                    directory: None,
                    load: next,
                };
            }
            _ => {}
        }
    }
    pub(super) fn clear_async_loads(&mut self) {
        match &mut self.backend {
            TabBackend::Local { load, enrich, .. } => {
                *load = None;
                *enrich = None;
            }
            TabBackend::Tags { load, .. } | TabBackend::Remote { load, .. } => *load = None,
        }
    }
    fn title(&self, cx: &App) -> String {
        self.path()
            .map(|path| {
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                )
            })
            .unwrap_or_else(|| self.location().label(cx))
    }

    pub(super) fn row_backing(&mut self) -> Arc<SelectionBacking<PathBuf>> {
        if let Some(backing) = &self.row_backing {
            return Arc::clone(backing);
        }
        let backing =
            SelectionBacking::from_items(self.table_rows.iter().map(|entry| entry.path.clone()));
        self.row_backing = Some(Arc::clone(&backing));
        backing
    }

    fn position_of_path(&self, path: &Path) -> Option<usize> {
        if let Some(backing) = &self.row_backing {
            return backing.position(path);
        }
        let picker_narrows = self
            .picker
            .as_ref()
            .is_some_and(picker::Projection::narrows);
        if self.sort.is_canonical()
            && self.filter().is_empty()
            && !picker_narrows
            && let Some(directory) = self.navigation.directory()
            && let Some(entry) = directory.entry_for_path(path)
        {
            return self
                .table_rows
                .binary_search_by(|candidate| {
                    (entry.kind == directory::EntryKind::Directory)
                        .cmp(&(candidate.kind == directory::EntryKind::Directory))
                        .then_with(|| candidate.name.cmp(&entry.name))
                        .then_with(|| candidate.path.as_os_str().cmp(entry.path.as_os_str()))
                })
                .ok();
        }
        self.table_rows.iter().position(|entry| entry.path == path)
    }

    pub(super) fn select_index_range(&mut self, start: usize, end: usize) {
        if self.table_rows.is_empty() {
            self.navigation.view_mut().selected.clear();
            return;
        }
        let (lo, hi) = (start.min(end), start.max(end));
        if hi >= self.table_rows.len() {
            self.navigation.view_mut().selected.clear();
            return;
        }
        if lo == hi {
            let path = self.table_rows[lo].path.clone();
            self.navigation.view_mut().selected.set_single(path);
            return;
        }
        if self.row_backing.is_some() || hi - lo + 1 >= RANGE_BACKING_MIN_SPAN {
            let backing = self.row_backing();
            self.navigation
                .view_mut()
                .selected
                .set_range(&backing, lo, hi);
        } else {
            let paths: Vec<PathBuf> = self.table_rows[lo..=hi]
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
            self.navigation.view_mut().selected.replace(paths);
        }
    }

    pub(super) fn resolve_cursor_index(&mut self) -> Option<usize> {
        let Some(path) = self.cursor.as_deref() else {
            self.cursor_index = None;
            return None;
        };
        if let Some(index) = self.cursor_index
            && self
                .table_rows
                .get(index)
                .is_some_and(|entry| entry.path.as_path() == path)
        {
            return Some(index);
        }
        let resolved = self.position_of_path(path);
        self.cursor_index = resolved;
        resolved
    }

    pub(super) fn resolve_anchor_index(&mut self) -> Option<usize> {
        let Some(path) = self.anchor.as_deref() else {
            self.anchor_index = None;
            return None;
        };
        if let Some(index) = self.anchor_index
            && self
                .table_rows
                .get(index)
                .is_some_and(|entry| entry.path.as_path() == path)
        {
            return Some(index);
        }
        let resolved = self.position_of_path(path);
        self.anchor_index = resolved;
        resolved
    }

    pub(super) fn resolve_cursor_or_selected_index(&mut self) -> Option<usize> {
        if self.cursor.is_some() {
            return self.resolve_cursor_index();
        }
        self.navigation
            .view()
            .selected
            .last()
            .and_then(|path| self.position_of_path(path))
    }

    /// Update `table_rows` in place after an enrichment batch whose changes cannot
    /// alter row membership or sort order, keeping `row_backing`, cursor/anchor index
    /// hints, and `selected` intact.
    ///
    /// Returns whether any updated entry lies within `viewport_range`.
    pub(super) fn apply_in_place_enrichment(
        &mut self,
        next_snapshot: &Arc<directory::DirectorySnapshot>,
        batch_updates: &[directory::Entry],
        viewport_range: Option<std::ops::Range<usize>>,
    ) -> bool {
        let picker_narrows = self
            .picker
            .as_ref()
            .is_some_and(picker::Projection::narrows);
        let unfiltered = self.filter().is_empty() && !picker_narrows;
        if unfiltered && self.sort.is_canonical() {
            self.sorted_projection = None;
            self.table_rows = Arc::clone(&next_snapshot.entries);
            let Some(range) = viewport_range else {
                return true;
            };
            if let Some(backing) = &self.row_backing {
                return batch_updates.iter().any(|update| {
                    backing
                        .position(update.path.as_path())
                        .is_some_and(|index| range.contains(&index))
                });
            }
            return batch_updates.iter().any(|update| {
                self.table_rows
                    .binary_search_by(|candidate| {
                        (update.kind == directory::EntryKind::Directory)
                            .cmp(&(candidate.kind == directory::EntryKind::Directory))
                            .then_with(|| candidate.name.cmp(&update.name))
                            .then_with(|| candidate.path.as_os_str().cmp(update.path.as_os_str()))
                    })
                    .is_ok_and(|index| range.contains(&index))
            });
        }

        let share_unfiltered_proj = unfiltered
            && self.sorted_projection.as_ref().is_some_and(|proj| {
                proj.sort == self.sort && Arc::ptr_eq(&proj.entries, &self.table_rows)
            });
        if share_unfiltered_proj && let Some(proj) = &mut self.sorted_projection {
            proj.entries = Arc::from([]);
        }

        let mut touched_viewport = viewport_range.is_none();
        let updates_by_path: Option<std::collections::HashMap<&Path, &directory::Entry>> =
            (self.row_backing.is_none() || !unfiltered).then(|| {
                batch_updates
                    .iter()
                    .map(|entry| (entry.path.as_path(), entry))
                    .collect()
            });

        if let Some(backing) = self.row_backing.clone() {
            if let Some(slice) = Arc::get_mut(&mut self.table_rows) {
                for update in batch_updates {
                    let Some(index) = backing.position(update.path.as_path()) else {
                        continue;
                    };
                    let Some(existing) = slice.get_mut(index) else {
                        continue;
                    };
                    if existing.ptr_eq(update) {
                        continue;
                    }
                    let visible_changed = &*existing != update;
                    *existing = update.clone();
                    if visible_changed
                        && let Some(range) = &viewport_range
                        && range.contains(&index)
                    {
                        touched_viewport = true;
                    }
                }
            } else {
                let mut rows: Option<Vec<directory::Entry>> = None;
                for update in batch_updates {
                    let Some(index) = backing.position(update.path.as_path()) else {
                        continue;
                    };
                    let Some(existing) = self.table_rows.get(index) else {
                        continue;
                    };
                    if existing.ptr_eq(update) {
                        continue;
                    }
                    let visible_changed = existing != update;
                    rows.get_or_insert_with(|| self.table_rows.iter().cloned().collect())[index] =
                        update.clone();
                    if visible_changed
                        && let Some(range) = &viewport_range
                        && range.contains(&index)
                    {
                        touched_viewport = true;
                    }
                }
                if let Some(rows) = rows {
                    self.table_rows = rows.into();
                }
            }
        } else if let Some(updates_by_path) = &updates_by_path {
            if let Some(slice) = Arc::get_mut(&mut self.table_rows) {
                for (index, existing) in slice.iter_mut().enumerate() {
                    let Some(update) = updates_by_path.get(existing.path.as_path()) else {
                        continue;
                    };
                    if existing.ptr_eq(update) {
                        continue;
                    }
                    let visible_changed = &*existing != *update;
                    *existing = (*update).clone();
                    if visible_changed
                        && let Some(range) = &viewport_range
                        && range.contains(&index)
                    {
                        touched_viewport = true;
                    }
                }
            } else {
                let mut rows: Option<Vec<directory::Entry>> = None;
                for (index, existing) in self.table_rows.iter().enumerate() {
                    let Some(update) = updates_by_path.get(existing.path.as_path()) else {
                        continue;
                    };
                    if existing.ptr_eq(update) {
                        continue;
                    }
                    let visible_changed = existing != *update;
                    rows.get_or_insert_with(|| self.table_rows.iter().cloned().collect())[index] =
                        (*update).clone();
                    if visible_changed
                        && let Some(range) = &viewport_range
                        && range.contains(&index)
                    {
                        touched_viewport = true;
                    }
                }
                if let Some(rows) = rows {
                    self.table_rows = rows.into();
                }
            }
        }

        if let Some(proj) = &mut self.sorted_projection
            && proj.sort == self.sort
        {
            if unfiltered {
                proj.source = Arc::clone(&next_snapshot.entries);
                proj.entries = Arc::clone(&self.table_rows);
            } else if let Some(updates_by_path) = &updates_by_path {
                if let Some(slice) = Arc::get_mut(&mut proj.entries) {
                    for existing in slice.iter_mut() {
                        if let Some(update) = updates_by_path.get(existing.path.as_path())
                            && !existing.ptr_eq(update)
                        {
                            *existing = (*update).clone();
                        }
                    }
                } else {
                    let mut updated_entries: Option<Vec<directory::Entry>> = None;
                    for (index, existing) in proj.entries.iter().enumerate() {
                        if let Some(update) = updates_by_path.get(existing.path.as_path())
                            && !existing.ptr_eq(update)
                        {
                            updated_entries
                                .get_or_insert_with(|| proj.entries.iter().cloned().collect())
                                [index] = (*update).clone();
                        }
                    }
                    if let Some(updated_entries) = updated_entries {
                        proj.entries = updated_entries.into();
                    }
                }
                proj.source = Arc::clone(&next_snapshot.entries);
            }
        }
        touched_viewport
    }

    pub(super) fn refresh_visible_if_dirty(&mut self) {
        if self.projection_dirty {
            self.refresh_visible();
        }
    }

    fn refresh_visible(&mut self) {
        self.projection_dirty = false;
        self.row_backing = None;
        self.cursor_index = None;
        self.anchor_index = None;
        if !self.files_available() {
            self.table_rows = Arc::from([]);
            self.sorted_projection = None;
            return;
        }
        let pending = (!self.is_loading())
            .then(|| self.pending_selection.take())
            .flatten();
        let raw_filter = self.filter();
        let filter = (!raw_filter.is_empty()).then(|| raw_filter.to_lowercase());
        let filter_str = filter.as_deref().unwrap_or("");
        let picker = self.picker.clone();
        let picker_narrows = picker.as_ref().is_some_and(picker::Projection::narrows);
        let Some(directory) = self.navigation.directory_arc() else {
            self.table_rows = Arc::from([]);
            self.sorted_projection = None;
            self.anchor = None;
            self.cursor = None;
            return;
        };
        let unfiltered = filter_str.is_empty() && !picker_narrows;
        if self.sort.is_canonical() {
            self.sorted_projection = None;
            if unfiltered {
                self.table_rows = Arc::clone(&directory.entries);
            } else {
                self.table_rows = directory
                    .entries
                    .iter()
                    .filter(|entry| entry.matches_filter(filter_str))
                    .filter(|entry| picker.as_ref().is_none_or(|picker| picker.keeps(entry)))
                    .cloned()
                    .collect::<Vec<_>>()
                    .into();
            }
        } else {
            let sorted = if let Some(proj) = &self.sorted_projection
                && Arc::ptr_eq(&proj.source, &directory.entries)
                && proj.sort == self.sort
            {
                Arc::clone(&proj.entries)
            } else {
                let entries = &directory.entries;
                let mut indices: Vec<usize> = (0..entries.len()).collect();
                self.sort.sort_indices(entries, &mut indices);
                let sorted: Arc<[directory::Entry]> = indices
                    .into_iter()
                    .map(|index| entries[index].clone())
                    .collect::<Vec<_>>()
                    .into();
                self.sorted_projection = Some(SortedProjection {
                    source: Arc::clone(&directory.entries),
                    sort: self.sort,
                    entries: Arc::clone(&sorted),
                });
                sorted
            };
            if unfiltered {
                self.table_rows = sorted;
            } else {
                self.table_rows = sorted
                    .iter()
                    .filter(|entry| entry.matches_filter(filter_str))
                    .filter(|entry| picker.as_ref().is_none_or(|picker| picker.keeps(entry)))
                    .cloned()
                    .collect::<Vec<_>>()
                    .into();
            }
        }

        if self.is_loading() {
            return;
        }

        let has_pending_reveal = self.pending_reveal.is_some();
        let needs_visibility_check = !self.navigation.view().selected.is_empty()
            || self.cursor.is_some()
            || self.anchor.is_some()
            || pending.is_some()
            || has_pending_reveal;
        if !needs_visibility_check {
            return;
        }

        let visible_paths: Option<std::collections::HashSet<&Path>> = (!unfiltered).then(|| {
            self.table_rows
                .iter()
                .map(|entry| entry.path.as_path())
                .collect()
        });
        let is_visible = |path: &Path| match &visible_paths {
            Some(set) => set.contains(path),
            None => directory.entry_for_path(path).is_some(),
        };

        // Hidden search results must never remain implicit targets of a file operation.
        self.navigation
            .view_mut()
            .selected
            .retain(|path| is_visible(path));
        if self.cursor.as_deref().is_some_and(|path| !is_visible(path)) {
            self.cursor = None;
        }
        if self.anchor.as_deref().is_some_and(|path| !is_visible(path)) {
            self.anchor = None;
        }
        if let Some(path) = pending
            && is_visible(&path)
        {
            self.navigation.view_mut().selected.set_single(path.clone());
            self.cursor = Some(path.clone());
            self.anchor = Some(path.clone());
            if let Some(index) = self.table_rows.iter().position(|entry| entry.path == path) {
                self.cursor_index = Some(index);
                self.anchor_index = Some(index);
                self.scroll_to_entry(index);
            }
        }
        if !self.is_loading() {
            if let Some(paths) = self.pending_reveal.take() {
                let selected: Vec<_> = paths.into_iter().filter(|path| is_visible(path)).collect();
                if let Some(path) = selected.first() {
                    self.cursor = Some(path.clone());
                    self.anchor = Some(path.clone());
                    if let Some(index) =
                        self.table_rows.iter().position(|entry| entry.path == *path)
                    {
                        self.cursor_index = Some(index);
                        self.anchor_index = Some(index);
                        self.scroll_to_entry(index);
                    }
                }
                self.navigation.view_mut().selected.replace(selected);
            }
        }
        if self.navigation.view().selected.is_range() {
            if !self.table_rows.is_empty()
                && self.navigation.view().selected.len() == self.table_rows.len()
            {
                self.select_index_range(0, self.table_rows.len() - 1);
            } else {
                self.navigation.view_mut().selected.compact();
            }
        }
    }

    fn scroll_to_entry(&self, index: usize) {
        let row = if self.view_mode == ViewMode::Grid {
            index / self.grid.columns
        } else {
            index
        };
        self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
    }

    fn save_scroll(&mut self) {
        let view = self.navigation.view_mut();
        view.scroll_offset = self.scroll.0.borrow().base_handle.offset().y.into();
    }

    fn restore_scroll(&self) {
        self.scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(self.navigation.view().scroll_offset)));
    }
}

/// The window composes feature-owned state; tab-local browsing stays in `OpenTab`.
pub struct Browser {
    pub(super) window: window::State,
    pub(super) settings: settings::State,
    pages: gds::ui::page::stack::Stack<pages::Page>,

    locations: locations::State,
    pub(super) tabs: tabs::State,
    sidebar: sidebar::State,
    pub(super) toolbar: toolbar::State,

    files: content::State,
    collections: place_pages::State,
    devices: devices::State,
    remote: remote::State,

    menus: menu::State,
    pub(super) operations: file_actions::State,
    background_tasks: Entity<background_tasks::State>,
    clipboard_details: Entity<clipboard_status::Details>,
    selection_details: Entity<selection_status::Details>,
    terminal: terminal::State,
    /// The picker session, when this window is a file picker.
    picker: Option<picker::State>,
}

impl Browser {
    pub fn new(options: LaunchOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let LaunchOptions {
            places,
            start,
            home,
            picker,
        } = options;
        let tasks = cx.gds().tasks().scope();
        let preferences = cx.gds().preferences().current();
        if picker.is_some() {
            // The picker's own decisions live in their own key scope, so they
            // cannot take Alt+A / Alt+C away from a dialog or the workspace.
            picker::register(cx);
        }
        notifications::bind_window(window, cx);
        let host = window::State::new(tasks.clone(), window, cx);
        let settings = settings::State::new(preferences, tasks.clone(), window, cx);
        let mut this = Self {
            locations: locations::State::new(home, places),
            tabs: tabs::State::default(),
            sidebar: sidebar::State::new(&settings.preferences, cx),
            toolbar: toolbar::State::new(window, cx),
            files: content::State::new(window, cx),
            collections: place_pages::State::new(window, cx),
            devices: devices::State::new(window, cx),
            remote: remote::State::new(host.tasks.clone(), cx),
            menus: menu::State::default(),
            operations: file_actions::State::new(window, cx),
            background_tasks: cx.new(|_| background_tasks::State::default()),
            clipboard_details: cx.new(|cx| clipboard_status::Details::new(host.tasks.clone(), cx)),
            selection_details: {
                let browser = cx.entity();
                cx.new(|cx| selection_status::Details::new(browser, cx))
            },
            terminal: terminal::State::new(&settings.preferences, cx),
            picker: picker.map(|options| picker::State::new(options, window, cx)),
            window: host,
            settings,
            pages: Default::default(),
        };
        Self::install_terminal_close_handler(window, cx);
        this.subscribe(window, cx);
        this.install_remote_events(cx);
        if let Some(error) = this.locations.load_error.clone() {
            // Reported once; `save_places` stays disabled so an unreadable database
            // is never overwritten with empty state.
            notifications::error(error, cx);
        }
        for location in this.locations.places.pinned.clone() {
            this.restore_pinned_tab(location, cx);
        }
        let start_location = this.resolve_location(&Location::from(&start), cx).location;
        if let Some((index, id)) = this
            .tabs
            .items
            .iter()
            .enumerate()
            .find(|(_, tab)| {
                tab.path() == Some(start.as_path()) || tab.location() == start_location
            })
            .map(|(index, tab)| (index, tab.id))
        {
            this.activate_tab_chrome(index, id, Some(window), cx);
            this.enter_location(cx);
        } else {
            this.open_tab(start, false, window, cx);
        }
        cx.gds_mut().clipboard().refresh();
        this.sync_workspace_state(cx);
        this
    }

    pub(super) fn current_tab(&self) -> Option<&OpenTab> {
        self.tab_slot(self.tabs.active_id)
    }
    fn current_tab_mut(&mut self) -> Option<&mut OpenTab> {
        self.tab_slot_mut(self.tabs.active_id)
    }
    fn collection(&self) -> Option<Collection> {
        self.current_tab()
            .and_then(|tab| tab.location().collection())
    }
    pub(super) fn active(&self) -> Option<&OpenTab> {
        self.current_tab().filter(|tab| tab.path().is_some())
    }
    fn active_mut(&mut self) -> Option<&mut OpenTab> {
        self.current_tab_mut().filter(|tab| tab.path().is_some())
    }
    fn tab_slot(&self, id: u64) -> Option<&OpenTab> {
        self.tabs.items.iter().find(|slot| slot.id == id)
    }
    fn tab_slot_mut(&mut self, id: u64) -> Option<&mut OpenTab> {
        self.tabs.items.iter_mut().find(|slot| slot.id == id)
    }

    fn width(&self) -> f32 {
        self.sidebar.presentation.width()
    }

    fn content_width(&self) -> f32 {
        let details = if self.details_visible() {
            view::PREVIEW_WIDTH
        } else {
            0.
        };
        (self.window.layout.width - self.width() - view::TABLE_INSET * 2. - details).max(1.)
    }

    pub(in crate::pages::workspace) fn sync_content_width(&mut self, cx: &mut Context<Self>) {
        if !self.pages.is_empty() {
            return;
        }
        let content_width = self.content_width();
        let font_scale = f32::from(cx.gds().appearance().typography().scaled(1.));
        let mode = self.view_mode();
        let is_collection = self.collection().is_some();
        if let Some(slot) = self.current_tab_mut() {
            let geometry = grid::Geometry::new(content_width, font_scale);
            let reflow = slot.view_mode != mode
                || slot.grid.columns != geometry.columns
                || slot.grid.row_height != geometry.row_height;
            slot.view_mode = mode;
            slot.grid = geometry;
            if !is_collection
                && reflow
                && let Some(index) = slot.resolve_cursor_index()
            {
                slot.scroll_to_entry(index);
            }
        }
        match self.collection() {
            None => {
                if let Some(slot) = self
                    .active()
                    .filter(|slot| slot.navigation.directory().is_some())
                {
                    let enrichment_need = self.enrichment_need(slot.id).unwrap_or_default();
                    let data = table::Data {
                        rows: slot.table_rows.clone(),
                        selected: slot.navigation.view().selected.clone(),
                        sort: slot.sort,
                        enrichment_need,
                        scroll: slot.scroll.clone(),
                        content_width,
                        layout: self.columns_layout(&self.current_location()).clone(),
                        location: self.current_location(),
                        widths: self.locations.places.column_widths(self.current_location()),
                    };
                    table::FileTable::sync_state(&self.files.table, data, cx);
                }
            }
            Some(collection @ (Collection::Recent | Collection::Favorites | Collection::Tags)) => {
                if let Some(locations) = self.place_locations(collection)
                    && let Some(table_state) = self.place_table(collection)
                {
                    let selected = self.selected_place();
                    place_pages::table::LocationTable::sync(
                        table_state,
                        locations,
                        selected,
                        content_width,
                        cx,
                    );
                }
            }
            Some(Collection::Network | Collection::Trash | Collection::Volumes) => {}
        }
    }

    fn sync_workspace_state(&mut self, cx: &mut Context<Self>) {
        if !self.pages.is_empty() {
            self.menus.open = None;
            if self.clipboard_details.read(cx).is_open() {
                self.clipboard_details
                    .update(cx, |details, cx| details.set_open(false, cx));
            }
            if self.selection_details.read(cx).is_open() {
                let browser = cx.entity();
                self.selection_details
                    .update(cx, |details, cx| details.set_open(false, &browser, cx));
            }
        }
        if !self.pages.is_empty() || self.collection().is_some() {
            self.menus.sort = None;
        }
        if !self.pages.is_empty()
            || self.collection().is_some()
            || self.menus.is_any_open()
            || self.picker.is_some()
        {
            self.discard_marquee();
        }
        if self.pages.is_empty() {
            let sort = columns::sort_for(&self.current_location(), &self.locations.places);
            let is_collection = self.collection().is_some();
            let should_refresh = !is_collection
                && self
                    .current_tab()
                    .is_some_and(|slot| slot.sort != sort || slot.projection_dirty);
            if should_refresh {
                self.release_active_selection_mirrors(cx);
            }
            let mut needs_enrich = false;
            if let Some(slot) = self.current_tab_mut()
                && !is_collection
            {
                if slot.sort != sort {
                    slot.sort = sort;
                    slot.refresh_visible();
                    needs_enrich = true;
                } else if slot.projection_dirty {
                    slot.refresh_visible_if_dirty();
                    needs_enrich = true;
                }
            }
            if needs_enrich {
                self.schedule_enrichment(self.tabs.active_id, cx);
            }
            if matches!(
                self.collection(),
                None | Some(Collection::Recent | Collection::Favorites | Collection::Tags)
            ) {
                self.sync_preview(cx);
            }
            self.sync_content_width(cx);
        }
        self.sync_sidebar_split(cx);
        self.sync_terminal_cwd(cx);
        self.drain_device_removals(cx);
    }

    pub(super) fn current_location(&self) -> Location {
        self.current_tab()
            .map_or_else(|| Location::from(&self.locations.home), OpenTab::location)
    }

    fn current_place_location(&self) -> Location {
        self.current_path()
            .as_ref()
            .map(Location::from)
            .unwrap_or_else(|| self.current_location())
    }

    fn details_available(&self) -> bool {
        self.collection() != Some(Collection::Tags)
            || self
                .current_tab()
                .and_then(|slot| slot.tags())
                .and_then(|result| result.as_ref().ok())
                .is_some_and(|contents| !contents.entries.is_empty())
    }

    fn details_visible(&self) -> bool {
        self.details_available()
            && !self.window.layout.compact
            && self
                .locations
                .places
                .details_visible(self.current_location())
    }

    pub(super) fn current_path(&self) -> Option<PathBuf> {
        self.active().and_then(OpenTab::path).map(Path::to_path_buf)
    }

    fn picker_projection(&self) -> Option<picker::Projection> {
        self.picker
            .as_ref()
            .map(|picker| picker.session.projection())
    }

    pub(super) fn open_tab(
        &mut self,
        path: PathBuf,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_tab_inner(path, pinned, Some(window), cx);
    }

    fn open_tab_inner(
        &mut self,
        path: PathBuf,
        pinned: bool,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.open_location_tab(Location::from(&path), pinned, window, cx);
    }

    fn append_location_tab(
        &mut self,
        location: Location,
        pinned: bool,
        cx: &App,
    ) -> (usize, u64, Option<PathBuf>) {
        let resolved = self.resolve_location(&location, cx);
        let id = self.tabs.next_id;
        self.tabs.next_id += 1;
        let mut slot = OpenTab::new(id, resolved.location, resolved.path.clone(), pinned);
        slot.set_device(resolved.device);
        slot.picker = self.picker_projection();
        self.tabs.items.push(slot);
        (self.tabs.items.len() - 1, id, resolved.path)
    }

    fn activate_tab_chrome(
        &mut self,
        index: usize,
        id: u64,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.tabs.active_id = id;
        self.pages.clear();
        self.toolbar.finish_path_edit();
        self.sidebar.regions.tabs_scroll.scroll_to_item(index);
        if let Some(window) = window {
            self.sync_filter_input(window, cx);
            self.window.focus.focus(window, cx);
        }
    }

    fn start_initial_tab_load(
        &mut self,
        id: u64,
        path: Option<PathBuf>,
        record_recent: bool,
        cx: &mut Context<Self>,
    ) {
        let options = self.read_options(id);
        let Some(slot) = self.tab_slot_mut(id) else {
            return;
        };
        let request = slot
            .navigation
            .prepare_initial(path, options, record_recent);
        self.start_request(id, request, cx);
    }

    fn restore_pinned_tab(&mut self, location: Location, cx: &mut Context<Self>) {
        let (_, id, path) = self.append_location_tab(location, true, cx);
        self.start_initial_tab_load(id, path, false, cx);
    }

    fn open_location_tab(
        &mut self,
        location: Location,
        pinned: bool,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let (index, id, path) = self.append_location_tab(location, pinned, cx);
        self.activate_tab_chrome(index, id, window, cx);
        self.start_initial_tab_load(id, path, true, cx);
    }

    fn start_request(&mut self, id: u64, request: LoadRequest, cx: &mut Context<Self>) {
        self.watch_directory(id, request.path(), cx);
        if let Some(slot) = self.tab_slot_mut(id) {
            slot.clear_async_loads();
        }
        match crate::entities::location::scheme::browse(request.location()) {
            crate::entities::location::scheme::Browse::Tags(address) => {
                self.load_tags(id, request.clone(), address.clone(), cx);
                return;
            }
            crate::entities::location::scheme::Browse::Remote(address) => {
                self.load_remote(id, request.clone(), address.clone(), cx);
                return;
            }
            crate::entities::location::scheme::Browse::Location => (),
        }
        self.remote.requests.remove(&id);
        let expected = self.resolve_location(request.location(), cx).device;
        if request.path().is_none() {
            self.finish_request(id, &request, Ok(None), expected, cx);
            return;
        }
        let stream_options = self.settings.preferences.files.streaming.options();
        let cancel = self.window.tasks.token().child_token();
        let worker_cancel = cancel.clone();
        let worker = request.clone();
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<Arc<directory::DirectorySnapshot>>();
        let work = IoTask::cancel_on_drop(self.window.tasks.spawn_blocking_named(
            "Read directory",
            move || {
                worker.load_base_streaming(stream_options, &worker_cancel, |partial| {
                    let _ = progress_tx.send(partial);
                })
            },
        ));
        let task = cx.spawn(async move |this, cx| {
            while let Some(mut latest) = progress_rx.recv().await {
                while let Ok(newer) = progress_rx.try_recv() {
                    latest = newer;
                }
                let Ok(keep_going) = this.update(cx, |this, cx| {
                    this.apply_partial_request(id, &request, latest, expected.as_ref(), cx)
                }) else {
                    return;
                };
                if !keep_going {
                    return;
                }
            }
            let result = work
                .await
                .map_err(directory::Error::from)
                .and_then(std::convert::identity);
            if let Err(error) = this.update(cx, |this, cx| {
                this.finish_request(id, &request, result, expected, cx)
            }) {
                tracing::warn!(%error, "navigation completed after the window closed");
            }
        });
        if let Some(slot) = self.tab_slot_mut(id) {
            slot.set_local_load(Some(enrichment::LocalJob::new(cancel, task)));
        }
        cx.notify();
    }

    fn apply_partial_request(
        &mut self,
        id: u64,
        request: &LoadRequest,
        snapshot: Arc<directory::DirectorySnapshot>,
        expected: Option<&devices::DeviceLocation>,
        cx: &mut Context<Self>,
    ) -> bool {
        let valid = expected.is_none_or(|device| {
            device.availability != devices::Availability::Available
                || device.is_current(&self.devices.service.read(cx).snapshot)
        });
        let content_width = self.content_width();
        let font_scale = f32::from(cx.gds().appearance().typography().scaled(1.));
        let Some(slot) = self.tabs.items.iter_mut().find(|slot| slot.id == id) else {
            return false;
        };
        if !valid {
            if slot.navigation.is_current_request(request) {
                slot.set_local_load(None);
            }
            if slot.navigation.cancel_request(request) {
                notifications::error(i18n::text("device-path-unavailable", cx), cx);
            }
            cx.notify();
            return false;
        }
        let pin_changed = slot.pinned && slot.location() != *request.location();
        let first_commit = match slot.navigation.apply_partial_load(request, snapshot) {
            PartialLoadOutcome::Stale => return false,
            PartialLoadOutcome::FirstCommit => {
                slot.set_device(expected.cloned());
                slot.anchor = None;
                let location = slot.location();
                slot.sort = columns::sort_for(&location, &self.locations.places);
                slot.view_mode = self.locations.places.effective_view_mode(location);
                slot.grid = grid::Geometry::new(content_width, font_scale);
                slot.refresh_visible();
                slot.restore_scroll();
                true
            }
            PartialLoadOutcome::Updated => {
                slot.refresh_visible();
                false
            }
        };
        if first_commit {
            if pin_changed {
                self.sync_pinned();
                self.save_places(cx);
            }
            if id == self.tabs.active_id {
                self.request_filter_sync(cx);
                self.enter_location(cx);
            }
        }
        cx.notify();
        true
    }

    fn finish_request(
        &mut self,
        id: u64,
        request: &LoadRequest,
        result: Result<Option<Arc<directory::DirectorySnapshot>>, directory::Error>,
        expected: Option<devices::DeviceLocation>,
        cx: &mut Context<Self>,
    ) {
        let valid = expected.as_ref().is_none_or(|device| {
            device.availability != devices::Availability::Available
                || device.is_current(&self.devices.service.read(cx).snapshot)
        });
        let content_width = self.content_width();
        let font_scale = f32::from(cx.gds().appearance().typography().scaled(1.));
        let is_current = self
            .tab_slot(id)
            .is_some_and(|slot| slot.navigation.is_current_request(request));
        if is_current && valid && id == self.tabs.active_id {
            self.release_active_selection_mirrors(cx);
        }
        let Some(slot) = self.tabs.items.iter_mut().find(|slot| slot.id == id) else {
            return;
        };
        let had_partial = slot.navigation.had_partial_commit(request);
        if slot.navigation.is_current_request(request) {
            slot.set_local_load(None);
        }
        if !valid {
            if slot.navigation.cancel_request(request) {
                if had_partial {
                    slot.set_device(expected);
                    slot.refresh_visible();
                }
                notifications::error(i18n::text("device-path-unavailable", cx), cx);
            }
            cx.notify();
            return;
        }
        let pin_changed = slot.pinned && slot.location() != *request.location();
        let mut applied = false;
        match slot.navigation.finish_load(request, result) {
            Ok(LoadOutcome::Stale) => return,
            Ok(LoadOutcome::Applied) => {
                applied = true;
                if let Some(directory) = slot.navigation.directory() {
                    report_directory_warnings(directory, cx);
                }
                slot.set_device(expected);
                let location = slot.location();
                if !had_partial {
                    slot.anchor = None;
                    slot.sort = columns::sort_for(&location, &self.locations.places);
                }
                slot.view_mode = self.locations.places.effective_view_mode(location);
                slot.grid = grid::Geometry::new(content_width, font_scale);
                slot.refresh_visible();
                if !had_partial || slot.navigation.view().scroll_offset != 0. {
                    slot.restore_scroll();
                }
                let mut places_changed = false;
                if request.records_recent() {
                    let location = slot
                        .path()
                        .map(Location::from)
                        .unwrap_or_else(|| slot.location());
                    self.locations.places.record(location);
                    places_changed = true;
                }
                if pin_changed {
                    self.sync_pinned();
                    places_changed = true;
                }
                if places_changed {
                    self.save_places(cx);
                }
            }
            Err(error) if error.is_cancelled() => return,
            Err(error) => report_load_failure(error.to_string(), cx),
        }
        let watched_path = self
            .tab_slot(id)
            .and_then(OpenTab::path)
            .map(Path::to_path_buf);
        self.watch_directory(id, watched_path.as_deref(), cx);
        if applied {
            self.schedule_enrichment(id, cx);
        }
        if id == self.tabs.active_id && !had_partial {
            self.request_filter_sync(cx);
            self.enter_location(cx);
        }
        self.finish_pending_refresh(id, cx);
        cx.notify();
    }

    fn navigate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.navigate_tab(self.tabs.active_id, path, cx);
    }
    fn navigate_tab(&mut self, id: u64, path: PathBuf, cx: &mut Context<Self>) {
        self.navigate_tab_inner(id, path, false, cx);
    }
    fn navigate_tab_inner(
        &mut self,
        id: u64,
        path: PathBuf,
        explicit_device: bool,
        cx: &mut Context<Self>,
    ) {
        if !explicit_device && !self.device_path_available(&path, cx) {
            notifications::info(i18n::text("device-path-unavailable", cx), cx);
            return;
        }
        self.navigate_location(id, Location::from(&path), cx);
    }

    fn navigate_location(&mut self, id: u64, location: Location, cx: &mut Context<Self>) {
        if self
            .tab_slot(id)
            .and_then(|s| s.remote())
            .and_then(|r| r.view.as_ref())
            .is_some_and(|v| v.read(cx).busy())
        {
            notifications::info(i18n::text("remote-task-running", cx), cx);
            return;
        }
        let resolved = self.resolve_location(&location, cx);
        let options = self.read_options(id);
        self.save_tab_view(id, cx);
        let Some(slot) = self.tab_slot_mut(id) else {
            return;
        };
        let request = slot
            .navigation
            .prepare_open(resolved.location, resolved.path, options);
        if id == self.tabs.active_id {
            self.toolbar.finish_path_edit();
        }
        self.start_request(id, request, cx);
        cx.gds_mut().clipboard().refresh();
    }

    fn read_options(&self, id: u64) -> ReadOptions {
        ReadOptions {
            show_hidden: self.settings.preferences.files.show_hidden
                || self.tab_slot(id).is_some_and(|slot| slot.reveal_hidden),
        }
    }

    fn cycle_tab(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let targets = self.tab_cycle_targets();
        let target = if backwards {
            targets.previous
        } else {
            targets.next
        };
        if let Some(id) = target {
            self.switch_tab(id, window, cx);
        }
    }

    /// The tabs the page-cycle keys land on.
    ///
    /// The strip order is the cycle order, pinned tabs first, and both ends
    /// connect. A row advertises exactly this target, so a hint never names a
    /// tab the press would not really switch to.
    fn tab_cycle_targets(&self) -> TabCycleTargets {
        if !self.pages.is_empty() {
            return TabCycleTargets::default();
        }
        let ids: Vec<u64> = self.tabs.items.iter().map(|tab| tab.id).collect();
        TabCycleTargets {
            previous: cycle_neighbour(&ids, self.tabs.active_id, true),
            next: cycle_neighbour(&ids, self.tabs.active_id, false),
        }
    }

    /// Whether the close controls are revealed by the held modifier.
    ///
    /// Only the Ctrl group counts: Alt alone, and the Ctrl+Alt mix that AltGr
    /// reports, keep the close controls hidden just as they keep Ctrl hints off.
    fn revealing_tab_close(window: &Window) -> bool {
        Self::current_held_modifier(window) == Some(ui::HeldModifier::Ctrl)
    }

    /// The modifier group the window holds right now, or none while it is out of
    /// focus: a release outside this window is never delivered here.
    fn current_held_modifier(window: &Window) -> Option<ui::HeldModifier> {
        if !window.is_window_active() {
            return None;
        }
        ui::HeldModifier::from_modifiers(window.modifiers())
    }

    fn switch_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab_slot(id).is_none() {
            return;
        }
        self.save_tab_view(self.tabs.active_id, cx);
        self.release_active_selection_mirrors(cx);
        self.tabs.active_id = id;
        self.pages.clear();
        self.toolbar.finish_path_edit();
        if let Some(index) = self.tabs.items.iter().position(|slot| slot.id == id) {
            self.sidebar.regions.tabs_scroll.scroll_to_item(index);
        }
        if let Some(slot) = self.current_tab_mut() {
            slot.refresh_visible_if_dirty();
        }
        self.sync_filter_input(window, cx);
        self.refresh_if_hidden_changed(cx);
        self.schedule_enrichment(id, cx);
        self.enter_location(cx);
        cx.gds_mut().clipboard().refresh();

        self.window.focus.focus(window, cx);
        cx.notify();
    }

    /// Refresh the visible folder when it was read with another hidden-file
    /// setting than the current preference.
    fn refresh_if_hidden_changed(&mut self, cx: &mut Context<Self>) {
        let refresh_hidden = self
            .active()
            .map(|slot| &slot.navigation)
            .is_some_and(|tab| {
                tab.directory()
                    .is_some_and(|directory| directory.options.show_hidden)
                    != (self.settings.preferences.files.show_hidden
                        || self.active().is_some_and(|slot| slot.reveal_hidden))
            });
        if refresh_hidden {
            self.refresh_current(cx);
        }
    }

    fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_slot(id) else {
            return;
        };
        if tab.pinned {
            self.confirm_close_pinned_tab(id, window, cx);
        } else {
            self.close_tab_approved(id, window, cx);
        }
    }

    fn close_tab_approved(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.items.iter().position(|tab| tab.id == id) else {
            return;
        };
        let was_active = self.tabs.active_id == id;
        self.tabs.items.remove(index);
        if self.tabs.items.is_empty() {
            self.open_tab(self.locations.home.clone(), false, window, cx);
        } else if was_active {
            self.release_active_selection_mirrors(cx);
            self.tabs.active_id = self.tabs.items[index.min(self.tabs.items.len() - 1)].id;
            if let Some(slot) = self.current_tab_mut() {
                slot.refresh_visible_if_dirty();
            }
            self.schedule_enrichment(self.tabs.active_id, cx);
        }
        self.toolbar.finish_path_edit();
        self.sync_filter_input(window, cx);
        self.window.focus.focus(window, cx);
        self.sync_pinned();
        self.save_places(cx);
        cx.notify();
    }

    fn toggle_pin(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(slot) = self.tabs.items.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        slot.pinned = !slot.pinned;
        self.tabs.items.sort_by_key(|slot| !slot.pinned);
        self.sync_pinned();
        self.save_places(cx);
        cx.notify();
    }

    fn toggle_favorite(&mut self, cx: &mut Context<Self>) {
        self.locations
            .places
            .toggle_favorite(self.current_place_location());
        self.save_places(cx);
        cx.notify();
    }

    /// Toggle the bookmark of the visible location from the keyboard.
    ///
    /// `Ctrl+D` works in both directions: it adds the location when it is not
    /// bookmarked and removes it when it is, exactly like the star button.
    fn favorite_current_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pages.is_empty()
            || self.current_tab().is_none_or(|slot| slot.is_loading())
            || self.editing_input_focused(window, cx)
        {
            return;
        }
        self.toggle_favorite(cx);
    }

    fn activate_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.device_path_available(&path, cx) {
            return;
        }
        // A picker never hands a path to another program: folders navigate and
        // one regular file is what the window reports.
        if self.picker.is_some() {
            self.activate_picker_path(path, cx);
            return;
        }
        self.open_path(path, desktop::OpenMode::DefaultApplication, cx);
    }

    fn open_path(&mut self, path: PathBuf, mode: desktop::OpenMode, cx: &mut Context<Self>) {
        enum Activation {
            Directory,
            File(desktop::OpenOutcome),
        }

        let target_tab = self.tabs.active_id;
        let worker_path = path.clone();
        let desktop = self.window.desktop.clone();
        let work = IoTask::cancel_on_drop(self.window.tasks.spawn_cancellable_named(
            "Open selected files",
            async move {
                let metadata = tokio::fs::metadata(&worker_path).await.map_err(|source| {
                    desktop::Error::Io {
                        path: worker_path.clone(),
                        source,
                    }
                })?;
                if metadata.is_dir() && mode == desktop::OpenMode::DefaultApplication {
                    return Ok::<Activation, desktop::Error>(Activation::Directory);
                }
                desktop
                    .open_file(&worker_path, mode)
                    .await
                    .map(Activation::File)
            },
        ));
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(|error| format!("could not complete the open task: {error}"))
                .and_then(|result| result.map_err(|error| error.to_string()));
            if let Err(error) = this.update(cx, |this, cx| {
                match result {
                    Ok(Activation::Directory) => this.navigate_tab(target_tab, path, cx),
                    Ok(Activation::File(desktop::OpenOutcome::Opened)) => {
                        this.locations.places.record(&path);
                        this.save_places(cx);
                    }
                    Ok(Activation::File(desktop::OpenOutcome::Cancelled)) => {}
                    Err(error) => {
                        notifications::Report::new(notifications::Level::Error, error)
                            .replacing::<OpenNotice>()
                            .show(cx);
                    }
                }
                cx.notify();
            }) {
                tracing::warn!("Open result arrived after the view closed: {error}");
            }
        })
        .detach();
    }

    fn open_external_terminal(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.device_path_available(&path, cx) {
            return;
        }
        let desktop = self.window.desktop.clone();
        let work = self
            .window
            .tasks
            .spawn_blocking_named("Open external terminal", move || {
                desktop.open_terminal(&path)
            });
        cx.spawn(async move |this, cx| {
            if let Err(error) = work
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()))
            {
                if let Err(closed) = this.update(cx, |_, cx| {
                    notifications::Report::new(notifications::Level::Error, error.to_string())
                        .replacing::<TerminalNotice>()
                        .show(cx);
                    cx.notify();
                }) {
                    tracing::warn!(
                        "Terminal launch failed after the view closed: {error}; {closed}"
                    );
                }
            }
        })
        .detach();
    }

    fn refresh_current(&mut self, cx: &mut Context<Self>) {
        if self
            .collection()
            .is_some_and(|collection| collection != Collection::Tags)
        {
            return;
        }
        self.refresh_tab(self.tabs.active_id, cx);
    }

    fn refresh_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(location) = self.tab_slot(id).map(OpenTab::location) else {
            return;
        };
        let resolved = self.resolve_location(&location, cx);
        let options = self.read_options(id);
        self.save_tab_view(id, cx);
        let Some(slot) = self.tab_slot_mut(id) else {
            return;
        };
        let request = slot.navigation.prepare_refresh(resolved.path, options);
        self.start_request(id, request, cx);
    }

    fn finish_pending_refresh(&mut self, id: u64, cx: &mut Context<Self>) {
        let refresh = self
            .tab_slot_mut(id)
            .is_some_and(|slot| std::mem::take(&mut slot.needs_refresh));
        if refresh {
            self.refresh_tab(id, cx);
        }
    }

    fn refresh_after_file_operation(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        self.devices
            .service
            .update(cx, |service, cx| service.refresh_capacity(cx));
        let parents: std::collections::HashSet<_> =
            paths.iter().filter_map(|path| path.parent()).collect();
        let mut ready = Vec::new();
        for slot in &mut self.tabs.items {
            let affected = slot.path().is_some_and(|current| {
                parents.contains(current) || paths.iter().any(|path| current.starts_with(path))
            });
            if !affected {
                continue;
            }
            if slot.is_loading() {
                slot.needs_refresh = true;
            } else {
                ready.push(slot.id);
            }
        }
        for id in ready {
            self.refresh_tab(id, cx);
        }
        cx.notify();
    }

    fn dispatch(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        // Window toggles and notification access may cross the PTY focus
        // boundary; file actions and cancellation must stay in their workspace.
        if self.terminal_focused(window, cx)
            && !matches!(
                command,
                Command::Settings
                    | Command::Help
                    | Command::Terminal
                    | Command::FocusNotifications
                    | Command::CloseNotification
                    | Command::CloseAllNotifications
            )
        {
            cx.propagate();
            return;
        }
        // A dialog owns focus; its own Enter/Esc handling and buttons remain
        // active, but workspace shortcuts must not run behind it.
        if self.operations.rename.read(cx).is_open() || window.has_active_dialog(cx) {
            return;
        }
        if !self.pages.is_empty() {
            if matches!(command, Command::Cancel | Command::Back | Command::CloseTab) {
                self.back_page(window, cx);
                return;
            }
            if !matches!(
                command,
                Command::Settings
                    | Command::Help
                    | Command::Sidebar
                    | Command::Terminal
                    | Command::FocusNotifications
                    | Command::CloseNotification
                    | Command::CloseAllNotifications
            ) && !(self.pages.current() == Some(&pages::Page::Settings)
                && matches!(command, Command::NextTab | Command::PreviousTab))
            {
                return;
            }
        }
        if command == Command::Cancel
            && let Some(cancel) = &self.operations.remote_paste_cancel
        {
            cancel.cancel();
            return;
        }
        if notifications::is_focused(window, cx)
            && !matches!(
                command,
                Command::FocusLocations
                    | Command::FocusVolumes
                    | Command::FocusTabs
                    | Command::FocusContent
                    | Command::FocusNotifications
                    | Command::CloseNotification
                    | Command::CloseAllNotifications
                    | Command::Menu
                    | Command::Settings
                    | Command::Help
                    | Command::Terminal
                    | Command::Sidebar
                    | Command::Cancel
            )
        {
            return;
        }
        // Window actions can win their bindings before popup key listeners.
        // Close the controlled menu before handling its chosen command.
        if self.menus.dismiss_for_command(command) {
            self.window.focus.focus(window, cx);
            if command == Command::Cancel {
                cx.notify();
                return;
            }
        }
        if self
            .current_tab()
            .is_some_and(|slot| slot.navigation.is_loading())
        {
            if command == Command::Cancel {
                self.cancel_location_load(cx);
                return;
            }
            if matches!(
                command,
                Command::Open
                    | Command::NewFolder
                    | Command::OpenInNewTab
                    | Command::Copy
                    | Command::Cut
                    | Command::Paste
                    | Command::Rename
                    | Command::Trash
                    | Command::DeletePermanent
                    | Command::SelectAll
                    | Command::DeselectAll
            ) {
                return;
            }
        }
        if self.pages.is_empty()
            && !self.editing_input_focused(window, cx)
            && !window.has_active_dialog(cx)
        {
            if let Some(view) = self.remote_view() {
                if view.update(cx, |view, cx| view.command(command, window, cx)) {
                    return;
                }
            }
        }
        match command {
            Command::FocusLocations
            | Command::FocusVolumes
            | Command::FocusTabs
            | Command::FocusContent => {
                self.focus_region(command, window, cx);
            }
            Command::FocusNotifications => notifications::focus(window, cx),
            Command::CloseNotification => notifications::close_current(window, cx),
            Command::CloseAllNotifications => notifications::close_all(window, cx),
            Command::Menu => {
                // Settings keeps its own way back, and the path editor's text
                // lives only in its widget: opening the menu there would discard
                // it. Every location page keeps the menu.
                if !self.pages.is_empty() || self.editing_path(window, cx) {
                    return;
                }
                if self.menus.open == Some(menu::Kind::Application) {
                    // The menu is about to be unmounted; take focus back before
                    // its own handle disappears.
                    self.menus.open = None;
                    self.window.focus.focus(window, cx);
                } else {
                    self.menus.open = Some(menu::Kind::Application);
                }
            }
            Command::Help => self.show_page(pages::Page::Help, window, cx),
            Command::Settings => {
                // The content region is about to unmount. Keep window actions
                // reachable until a settings control takes focus.
                self.show_page(pages::Page::Settings, window, cx);
                self.toolbar.finish_path_edit();
                self.settings.view.update(cx, |view, cx| {
                    view.set_preferences(self.settings.preferences.clone(), window, cx);
                    view.refresh_themes(window, cx);
                });
            }
            Command::NewFolder => {
                if let Some(path) = self.current_path() {
                    self.request_new_folder(path, window, cx);
                }
            }
            Command::NewWindow => {
                if self.picker.is_none() {
                    cx.emit(NewWindowRequested(
                        self.current_path()
                            .unwrap_or_else(|| self.locations.home.clone()),
                    ));
                }
            }
            Command::NewTab => self.open_tab(self.locations.home.clone(), false, window, cx),
            Command::CloseTab => {
                // Only an ordinary visible tab can claim Ctrl+W.
                if self
                    .tabs
                    .items
                    .iter()
                    .any(|tab| tab.id == self.tabs.active_id)
                {
                    self.close_tab(self.tabs.active_id, window, cx);
                }
            }
            Command::NextTab | Command::PreviousTab => {
                let backwards = matches!(command, Command::PreviousTab);
                if self.pages.current() == Some(&pages::Page::Settings) {
                    self.settings
                        .view
                        .update(cx, |view, cx| view.cycle_page(backwards, window, cx));
                } else {
                    self.cycle_tab(backwards, window, cx);
                }
            }
            Command::Sidebar => {
                if !self.window.layout.compact {
                    self.sidebar.collapsed = !self.sidebar.collapsed;
                }
            }
            Command::Terminal => self.toggle_terminal(window, cx),
            Command::EditLocation => {
                let path = self
                    .current_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.current_location().display_address());
                self.toolbar.start_path_edit(path, window, cx);
            }
            Command::Back | Command::Forward | Command::Parent => {
                self.navigate_history(command, cx)
            }
            Command::Refresh => {
                self.devices
                    .service
                    .update(cx, |service, cx| service.refresh(cx));
                if self.collection() == Some(Collection::Trash) {
                    self.refresh_trash(cx);
                } else if self.collection() == Some(Collection::Network) {
                    self.refresh_network(cx);
                } else if self.collection() == Some(Collection::Favorites) {
                    self.refresh_favorites(cx);
                } else {
                    self.refresh_current(cx);
                }
            }
            Command::Filter => {
                if self.pages.is_empty()
                    && (self.collection().is_none() || self.place_collection().is_some())
                {
                    self.toolbar.filter.input.update(cx, |input, cx| {
                        input.focus(window, cx);
                        input.select_all(window, cx);
                    });
                }
            }
            Command::Undo | Command::Redo => {
                if !self.editing_input_focused(window, cx) {
                    self.replay_file_operation(command, window, cx);
                }
            }
            Command::Copy | Command::Cut => {
                if command == Command::Copy
                    && matches!(
                        self.collection(),
                        Some(Collection::Favorites | Collection::Network | Collection::Tags)
                    )
                    && self.list_keys_available(window, cx)
                    && let Some(path) = self.selected_place_path()
                {
                    self.copy_file_paths(vec![path], false, cx);
                } else if self.file_selection_focused(window, cx) {
                    self.copy_files(command == Command::Cut, window, cx);
                }
            }
            Command::Paste => {
                if self.file_selection_focused(window, cx) {
                    if let Some(path) = self.current_path() {
                        self.paste_files(path, window, cx);
                    }
                }
            }
            Command::CopyPaths => {
                if self.list_keys_available(window, cx)
                    && let Some(location) = self.selected_place()
                {
                    self.copy_place_address(&location, cx);
                } else if self.file_selection_focused(window, cx) {
                    let paths = self.selected_visible_paths();
                    if !paths.is_empty() {
                        self.copy_selected_paths(&paths, cx);
                    }
                }
            }
            Command::OpenInNewTab => {
                if self.list_keys_available(window, cx)
                    && let Some(location) = self.selected_place()
                {
                    self.open_location_tab(location, false, window.into(), cx);
                } else if self.file_selection_focused(window, cx) {
                    self.open_selected_in_new_tabs(window, cx);
                }
            }
            Command::SelectAll => {
                if self.collection() == Some(Collection::Trash) {
                    self.trash_select_all(cx);
                } else {
                    self.select_all_files(window, cx);
                }
            }
            Command::DeselectAll => {
                if self.list_keys_available(window, cx) {
                    self.clear_list_selection(window, cx);
                }
            }
            Command::ContextMenu => self.open_selection_menu(window, cx),
            Command::Rename => {
                if self.collection() == Some(Collection::Volumes) {
                    if self.list_keys_available(window, cx)
                        && let Some(id) = self.volume_action_target(cx)
                    {
                        self.request_volume_rename(id, window, cx);
                    }
                } else if self.file_selection_focused(window, cx) {
                    self.request_rename(window, cx);
                }
            }
            Command::Details => {
                if self.details_available()
                    && !self.window.layout.compact
                    && self.pages.is_empty()
                    && (self.collection().is_none() || self.place_collection().is_some())
                    && !window.has_active_dialog(cx)
                {
                    let visible = !self.details_visible();
                    self.locations
                        .places
                        .set_details(self.current_location(), visible);
                    self.save_places(cx);
                    if !visible {
                        self.files
                            .preview
                            .update(cx, |preview, cx| preview.clear(cx));
                    } else {
                        self.schedule_enrichment(self.tabs.active_id, cx);
                    }
                }
            }
            Command::ViewMode => {
                let mode = if self.grid_mode() {
                    ViewMode::List
                } else {
                    ViewMode::Grid
                };
                self.set_view_mode(mode, window, cx);
            }
            Command::Sort => self.set_sort_menu_open(self.menus.sort.is_none(), window, cx),
            Command::Columns => {
                if !self.editing_input_focused(window, cx)
                    && self.collection().is_none()
                    && !self.grid_mode()
                {
                    self.open_columns_editor(window, cx);
                }
            }
            Command::Favorite => {
                if matches!(
                    self.collection(),
                    Some(
                        Collection::Recent
                            | Collection::Favorites
                            | Collection::Network
                            | Collection::Tags
                    )
                ) && self.list_keys_available(window, cx)
                    && self.selected_place().is_some()
                {
                    self.favorite_selected_place(false, cx);
                } else {
                    self.favorite_current_location(window, cx);
                }
            }
            Command::Trash => {
                if self.collection() == Some(Collection::Tags)
                    && self.list_keys_available(window, cx)
                {
                    self.request_delete_tag(window, cx);
                } else if self.collection() == Some(Collection::Favorites)
                    && self.list_keys_available(window, cx)
                {
                    self.favorite_selected_place(true, cx);
                } else if self.collection() == Some(Collection::Trash) {
                    // Delete on a trashed record is a permanent deletion, not a
                    // second move into Trash; it keeps the confirmation step.
                    self.request_trash_purge(window, cx);
                } else if self.file_selection_focused(window, cx) {
                    self.request_trash_selection(window, cx);
                }
            }
            Command::DeletePermanent => {
                if self.collection() == Some(Collection::Tags)
                    && self.list_keys_available(window, cx)
                {
                    self.request_delete_tag(window, cx);
                } else if self.collection() == Some(Collection::Trash) {
                    self.request_trash_purge(window, cx);
                } else if self.file_selection_focused(window, cx) {
                    self.request_delete_selection(window, cx);
                }
            }
            Command::RestoreSelection => {
                if self.collection() == Some(Collection::Trash) {
                    self.restore_selected_trash(cx);
                }
            }
            Command::PurgeSelection => {
                if self.collection() == Some(Collection::Trash) {
                    self.request_trash_purge(window, cx);
                }
            }
            Command::EmptyTrash => {
                if self.collection() == Some(Collection::Trash) {
                    self.request_empty_trash(window, cx);
                }
            }
            Command::ClearRecent => {
                if self.collection() == Some(Collection::Recent) {
                    self.clear_recent(cx);
                }
            }
            Command::MountVolume | Command::UnmountVolume => {
                self.run_volume_command(command, window, cx);
            }
            Command::Hidden => {
                self.edit_preferences(
                    |preferences| {
                        preferences.files.show_hidden = !preferences.files.show_hidden;
                    },
                    cx,
                );
            }
            Command::Open => {
                if self.sidebar.regions.sidebar_focused(window, cx) {
                    self.activate_region(window, cx);
                } else if self.collection() == Some(Collection::Volumes) {
                    if self.list_keys_available(window, cx)
                        && let Some(id) = self.volume_action_target(cx)
                    {
                        self.show_volume(id, window, cx);
                    }
                } else if matches!(
                    self.collection(),
                    Some(
                        Collection::Recent
                            | Collection::Favorites
                            | Collection::Network
                            | Collection::Tags
                    )
                ) {
                    // A places page opens its keyboard selection; the mouse
                    // keeps opening a row directly.
                    self.open_place_cursor(cx);
                } else if self.file_selection_focused(window, cx)
                    && self.active().is_some_and(|slot| !slot.is_loading())
                    && let Some(path) = self
                        .active()
                        .and_then(|slot| {
                            slot.cursor
                                .as_ref()
                                .or_else(|| slot.navigation.view().selected.first())
                        })
                        .cloned()
                {
                    self.activate_path(path, cx);
                }
            }
            Command::Cancel => {
                if self.remote.connector.read(cx).connecting {
                    self.remote
                        .connector
                        .update(cx, |connector, cx| connector.cancel(cx));
                    return;
                }
                // GPUI dispatches bound actions before key listeners and stops
                // propagation on the action, so `on_file_key_down` never sees
                // the escape that cancels a pointer drag. Cancelling a drag ends
                // only the drag: the release must not transfer files and the
                // selection stays intact.
                drag_drop::cancel_tab_hover(cx);
                if !cx.stop_active_drag(window) {
                    if self.sidebar.regions.sidebar_focused(window, cx) {
                        if self.pages.is_empty() {
                            self.cancel_clipboard_cut(cx);
                        }
                        self.window.focus.focus(window, cx);
                        cx.notify();
                        return;
                    }
                    if self.cancel_marquee(cx) {
                        self.window.focus.focus(window, cx);
                        return;
                    }
                    // Esc leaves a location page where it is: only the
                    // transient modes (settings, path editing) are cancelled.
                    // A picker leaves the window only once those are closed,
                    // so the first Esc still closes its own local modes.
                    let local_mode = !self.pages.is_empty()
                        || self.toolbar.path.editing
                        || notifications::is_focused(window, cx)
                        || self
                            .toolbar
                            .filter
                            .input
                            .read(cx)
                            .focus_handle(cx)
                            .is_focused(window);
                    if !local_mode && self.cancel_clipboard_cut(cx) {
                        self.window.focus.focus(window, cx);
                        cx.notify();
                        return;
                    }

                    self.toolbar.finish_path_edit();
                    self.pages.clear();
                    // Copy contents remain available after Escape.
                    cx.gds_mut().clipboard().refresh();
                    self.window.focus.focus(window, cx);
                    if self.picker.is_some() && !local_mode {
                        self.cancel_picker(cx);
                    }
                }
            }
        }
        cx.notify();
    }
}

/// The id a page-cycle key lands on in a tab strip of `ids`.
///
/// `ids` is the strip in cycle order, which is also the order pinned tabs sort
/// into. Both ends connect, and a strip of fewer than two tabs has no neighbour
/// to land on, so a single tab advertises no page-cycle key.
fn cycle_neighbour(ids: &[u64], active: u64, backwards: bool) -> Option<u64> {
    let count = ids.len();
    if count < 2 {
        return None;
    }
    let current = ids.iter().position(|id| *id == active)?;
    let index = if backwards {
        (current + count - 1) % count
    } else {
        (current + 1) % count
    };
    Some(ids[index])
}

/// Report a failed directory load once per completed attempt.
fn report_load_failure(error: String, cx: &mut App) {
    notifications::Report::new(notifications::Level::Error, i18n::text("load-failed", cx))
        .detail(error)
        .replacing::<LoadNotice>()
        .show(cx);
}

/// Report incomplete directory entries once when a load is applied, never
/// while rendering.
fn report_directory_warnings(snapshot: &directory::DirectorySnapshot, cx: &mut App) {
    if snapshot.warnings.is_empty() {
        return;
    }
    let mut report = notifications::Report::new(
        notifications::Level::Warning,
        i18n::count("warning-count", snapshot.warnings.len(), cx),
    );
    for warning in &snapshot.warnings {
        tracing::warn!(path = %warning.path.display(), operation = ?warning.operation,
            error_kind = ?warning.source.kind(), "{}", warning.source);
        let path = warning
            .path
            .as_os_str()
            .to_string_lossy()
            .escape_debug()
            .to_string();
        report = report.detail(format!("{path}: {}", warning.source));
    }
    report.replacing::<WarningNotice>().show(cx);
}

/// Report that preferences could not be applied or saved.
fn report_settings_failure(error: configuration::Error, cx: &mut App) {
    notifications::Report::new(
        notifications::Level::Error,
        i18n::t!(@cx, "settings-apply-failed", error = error.to_string()),
    )
    .replacing::<SettingsNotice>()
    .show(cx);
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
