use crate::entities::location::{Location, VolumeIdentity};
use gds::appearance::tokens;
mod background_tasks;
mod breadcrumb;
mod clipboard_status;
mod columns;
mod content;
mod context_menu;
#[cfg(target_os = "linux")]
mod desktop_requests;
mod devices;
mod drag_drop;
#[cfg(feature = "files-e2e")]
pub(crate) mod e2e;
mod entry_appearance;
mod file_actions;
mod file_icon;
mod file_label;
mod grid;
mod handlers;
mod history;
mod loading;
mod localization;
mod locations;
mod marquee;
mod menu;
mod motion;
mod name_form;
mod new_folder;
mod pages;
#[cfg(unix)]
mod permissions;
mod picker;
mod place_pages;
mod preview;
mod recycle;
mod regions;
mod remote;
mod rename;
mod responsive;
mod routing;
mod selection;
mod selection_status;
mod settings;
mod sidebar;
mod sort;
mod status;
mod table;
mod tabs;
mod tag_editor;
mod tags;
mod terminal;
mod toolbar;
mod transfers;
mod undo;
mod view;
mod watching;
mod window;

use gds::service::task_manager::Scope;
use gds::service::task_manager::Task as IoTask;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::entities::location::volumes as storage;
use crate::entities::preferences as configuration;
use crate::shared::ui::actions::*;
use gds::{i18n, service::desktop, ui, ui::notifications};
use gpui_kit::component::WindowExt;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use crate::entities::file::directory::{self, ReadOptions};
use crate::entities::file::{ViewMode, sorting};
use crate::entities::location::navigation::{self, LoadOutcome, LoadRequest, Navigation};
use crate::entities::location::places::Places;
use crate::shared::lib::clipboard as file_clipboard;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Collection {
    Tags,
    Recent,
    Favorites,
    Network,
    Trash,
    Volumes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LocationTarget {
    Tags,
    Home,
    Recent,
    Favorites,
    Volumes,
    Network,
    Trash,
}

pub struct NewWindowRequested(pub PathBuf);
impl EventEmitter<NewWindowRequested> for Browser {}

pub struct LaunchOptions {
    pub tasks: Scope,
    pub places: Result<Places, crate::entities::location::places::Error>,
    pub start: PathBuf,
    pub home: PathBuf,
    pub preferences: configuration::Preferences,
    /// Errors gathered before the window existed, reported once as toasts.
    pub startup_error: Option<String>,
    /// Present when the window is a file picker instead of a browser.
    pub picker: Option<crate::features::pick_files::Options>,
}

impl Collection {
    fn from_location(location: &Location) -> Option<Self> {
        match location {
            Location::Tags(_) => Some(Self::Tags),
            Location::Recent => Some(Self::Recent),
            Location::Favorites => Some(Self::Favorites),
            Location::Network => Some(Self::Network),
            Location::Trash => Some(Self::Trash),
            Location::Volumes => Some(Self::Volumes),
            _ => None,
        }
    }
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
struct ViewState {
    selected: Vec<PathBuf>,
    place: Option<Location>,
    scroll_offset: f32,
    filter: String,
    trash: recycle::ViewState,
    volume: Option<storage::VolumeId>,
}

struct OpenTab {
    tags: Option<Result<tags::Contents, String>>,
    tags_load: Option<Task<()>>,
    watch: Option<watching::Watch>,
    remote: Option<remote::Directory>,
    remote_load: Option<Task<()>>,
    id: u64,
    needs_refresh: bool,
    navigation: Navigation<ViewState>,
    filter: String,
    visible: Arc<[usize]>,
    table_rows: Arc<[directory::Entry]>,
    scroll: UniformListScrollHandle,
    view_mode: ViewMode,
    grid: grid::Geometry,
    pending_selection: Option<PathBuf>,
    pending_reveal: Option<Vec<PathBuf>>,
    reveal_hidden: bool,
    anchor: Option<PathBuf>,
    cursor: Option<PathBuf>,
    sort: sorting::Sort,
    pinned: bool,
    device: Option<devices::DeviceLocation>,
    /// The picker's type-filter projection for this tab, if this window is a
    /// picker. It is applied together with the name filter so a directory read
    /// never replaces the narrowed list with the unfiltered one.
    picker: Option<picker::Projection>,
}

impl OpenTab {
    fn new(id: u64, location: Location, path: Option<PathBuf>, pinned: bool) -> Self {
        Self {
            tags: None,
            tags_load: None,
            watch: None,
            remote: None,
            remote_load: None,
            id,
            navigation: Navigation::new(location, path),
            needs_refresh: false,
            filter: String::new(),
            visible: Arc::from([]),
            table_rows: Arc::from([]),
            scroll: UniformListScrollHandle::new(),
            view_mode: ViewMode::default(),
            grid: grid::Geometry::new(1., 1.),
            pending_selection: None,
            pending_reveal: None,
            reveal_hidden: false,
            anchor: None,
            cursor: None,
            sort: sorting::Sort::default(),
            pinned,
            device: None,
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

    fn refresh_visible(&mut self) {
        if !self.files_available() {
            self.visible = Arc::from([]);
            self.table_rows = Arc::from([]);
            return;
        }
        let pending = self.pending_selection.take();
        let filter = self.filter.to_lowercase();
        let picker = self.picker.clone();
        let Some(directory) = self.navigation.directory() else {
            self.visible = Arc::from([]);
            self.table_rows = Arc::from([]);
            self.anchor = None;
            self.cursor = None;
            return;
        };
        let entries = &directory.entries;
        let mut visible: Vec<_> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.display_name.to_lowercase().contains(&filter))
            .filter(|(_, entry)| picker.as_ref().is_none_or(|picker| picker.keeps(entry)))
            .map(|(index, _)| index)
            .collect();
        self.sort.sort_indices(entries, &mut visible);
        self.table_rows = visible
            .iter()
            .map(|&index| entries[index].clone())
            .collect::<Vec<_>>()
            .into();
        self.visible = visible.into();
        let visible_paths: std::collections::HashSet<_> =
            self.table_rows.iter().map(|entry| &entry.path).collect();
        // Hidden search results must never remain implicit targets of a file operation.
        self.navigation
            .view_mut()
            .selected
            .retain(|path| visible_paths.contains(path));
        if self
            .cursor
            .as_ref()
            .is_some_and(|path| !visible_paths.contains(path))
        {
            self.cursor = None;
        }
        if self
            .anchor
            .as_ref()
            .is_some_and(|path| !visible_paths.contains(path))
        {
            self.anchor = None;
        }
        if let Some(path) = pending
            && visible_paths.contains(&path)
        {
            self.navigation.view_mut().selected = vec![path.clone()];
            self.cursor = Some(path.clone());
            self.anchor = Some(path.clone());
            if let Some(index) = self.table_rows.iter().position(|entry| entry.path == path) {
                self.scroll_to_entry(index);
            }
        }
        if !self.is_loading() {
            if let Some(paths) = self.pending_reveal.take() {
                let selected: Vec<_> = paths
                    .into_iter()
                    .filter(|path| visible_paths.contains(path))
                    .collect();
                if let Some(path) = selected.first() {
                    self.cursor = Some(path.clone());
                    self.anchor = Some(path.clone());
                    if let Some(index) =
                        self.table_rows.iter().position(|entry| entry.path == *path)
                    {
                        self.scroll_to_entry(index);
                    }
                }
                self.navigation.view_mut().selected = selected;
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
        view.filter = self.filter.clone();
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
    window: window::State,
    settings: settings::State,
    pages: gds::ui::page::stack::Stack<pages::Page>,

    locations: locations::State,
    tabs: tabs::State,
    sidebar: sidebar::State,
    toolbar: toolbar::State,

    files: content::State,
    collections: place_pages::State,
    devices: devices::State,
    remote: remote::State,

    menus: menu::State,
    operations: file_actions::State,
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
            tasks,
            places,
            start,
            home,
            preferences,
            startup_error,
            picker,
        } = options;
        if picker.is_some() {
            // The picker's own decisions live in their own key scope, so they
            // cannot take Alt+A / Alt+C away from a dialog or the workspace.
            picker::register(cx);
        }
        notifications::bind_window(window, cx);
        // Root plugins are captured when this window's root is created, which
        // happens after the view is built, so the hint layer is installed here.
        ui::keytips::install(cx);
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
            clipboard_details: cx.new(|_| clipboard_status::Details::new(host.tasks.clone())),
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
        if let Some(error) = startup_error {
            notifications::error(error, cx);
        }
        if let Some(error) = this.locations.load_error.clone() {
            // Reported once; `save_places` stays disabled so an unreadable database
            // is never overwritten with empty state.
            notifications::error(error, cx);
        }
        for location in this.locations.places.pinned.clone() {
            this.open_location_tab(location, true, Some(window), cx);
        }
        if let Some(tab) = this
            .tabs
            .items
            .iter()
            .find(|tab| tab.path() == Some(start.as_path()))
        {
            this.tabs.active_id = tab.id;
        } else {
            this.open_tab(start, false, window, cx);
        }
        file_clipboard::refresh(cx);
        this
    }

    fn current_tab(&self) -> Option<&OpenTab> {
        self.tab_slot(self.tabs.active_id)
    }
    fn current_tab_mut(&mut self) -> Option<&mut OpenTab> {
        self.tab_slot_mut(self.tabs.active_id)
    }
    fn collection(&self) -> Option<Collection> {
        self.current_tab()
            .and_then(|tab| Collection::from_location(&tab.location()))
    }
    fn active(&self) -> Option<&OpenTab> {
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

    fn current_location(&self) -> Location {
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
                .and_then(|slot| slot.tags.as_ref())
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

    fn current_path(&self) -> Option<PathBuf> {
        self.active().and_then(OpenTab::path).map(Path::to_path_buf)
    }

    fn picker_projection(&self) -> Option<picker::Projection> {
        self.picker
            .as_ref()
            .map(|picker| picker.session.projection())
    }

    fn open_tab(
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

    fn open_location_tab(
        &mut self,
        location: Location,
        pinned: bool,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let resolved = self.resolve_location(&location, cx);
        let id = self.tabs.next_id;
        self.tabs.next_id += 1;
        let mut slot = OpenTab::new(id, resolved.location, resolved.path, pinned);
        slot.device = resolved.device;
        slot.picker = self.picker_projection();
        self.tabs.items.push(slot);
        self.tabs.active_id = id;
        self.pages.clear();
        self.toolbar.path.editing = false;
        self.sidebar
            .regions
            .tabs_scroll
            .scroll_to_item(self.tabs.items.len() - 1);
        if let Some(window) = window {
            self.sync_filter_input(window, cx);
            self.window.focus.focus(window, cx);
        }

        self.refresh_tab(id, cx);
    }

    fn start_request(&mut self, id: u64, request: LoadRequest, cx: &mut Context<Self>) {
        self.watch_directory(id, request.path(), cx);
        if let Some(slot) = self.tab_slot_mut(id) {
            slot.remote_load = None;
            slot.tags_load = None;
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
        let worker = request.clone();
        let work = self.window.tasks.spawn_blocking(move || worker.load());
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(directory::Error::from)
                .and_then(std::convert::identity);
            if let Err(error) = this.update(cx, |this, cx| {
                this.finish_request(id, &request, result, expected, cx)
            }) {
                tracing::warn!(%error, "navigation completed after the window closed");
            }
        })
        .detach();
        cx.notify();
    }

    fn finish_request(
        &mut self,
        id: u64,
        request: &LoadRequest,
        result: Result<Option<directory::DirectorySnapshot>, directory::Error>,
        expected: Option<devices::DeviceLocation>,
        cx: &mut Context<Self>,
    ) {
        let valid = expected.as_ref().is_none_or(|device| {
            device.availability != devices::Availability::Available
                || device.is_current(&self.devices.service.read(cx).snapshot)
        });
        let Some(slot) = self.tabs.items.iter_mut().find(|slot| slot.id == id) else {
            return;
        };
        if !valid {
            if slot.navigation.cancel_request(request) {
                notifications::error(i18n::text("device-path-unavailable", cx), cx);
            }
            return;
        }
        let pin_changed = slot.pinned && slot.location() != *request.location();
        match slot.navigation.finish_load(request, result) {
            Ok(LoadOutcome::Stale) => return,
            Ok(LoadOutcome::Applied) => {
                if !matches!(request.location(), Location::Tags(_)) {
                    slot.tags = None;
                }
                if !matches!(request.location(), Location::Remote(_)) {
                    slot.remote = None;
                }
                if let Some(directory) = slot.navigation.directory() {
                    report_directory_warnings(directory, cx);
                }
                slot.device = expected;
                slot.filter = slot.navigation.view().filter.clone();
                slot.anchor = None;
                slot.sort = columns::sort_for(&slot.location(), &self.locations.places);
                slot.refresh_visible();
                slot.restore_scroll();
                let location = slot
                    .path()
                    .map(Location::from)
                    .unwrap_or_else(|| slot.location());
                self.locations.places.record(location);
                if pin_changed {
                    self.sync_pinned();
                }
                self.save_places(cx);
            }
            Err(error) => report_load_failure(error.to_string(), cx),
        }
        let watched_path = self
            .tab_slot(id)
            .and_then(OpenTab::path)
            .map(Path::to_path_buf);
        self.watch_directory(id, watched_path.as_deref(), cx);
        if id == self.tabs.active_id {
            self.toolbar.filter.sync_pending = true;
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
            .and_then(|s| s.remote.as_ref())
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
            self.toolbar.path.editing = false;
        }
        self.start_request(id, request, cx);
        file_clipboard::refresh(cx);
    }

    fn read_options(&self, id: u64) -> ReadOptions {
        ReadOptions {
            show_hidden: self.settings.preferences.files.show_hidden
                || self.tab_slot(id).is_some_and(|slot| slot.reveal_hidden),
        }
    }

    fn cycle_tab(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (previous, next) = self.tab_cycle_targets();
        let target = if backwards { previous } else { next };
        if let Some(id) = target {
            self.switch_tab(id, window, cx);
        }
    }

    /// The tabs the page-cycle keys land on, as `(previous, next)`.
    ///
    /// The strip order is the cycle order, pinned tabs first, and both ends
    /// connect. A row advertises exactly this target, so a hint never names a
    /// tab the press would not really switch to.
    fn tab_cycle_targets(&self) -> (Option<u64>, Option<u64>) {
        if !self.pages.is_empty() {
            return (None, None);
        }
        let ids: Vec<u64> = self.tabs.items.iter().map(|tab| tab.id).collect();
        (
            cycle_neighbour(&ids, self.tabs.active_id, true),
            cycle_neighbour(&ids, self.tabs.active_id, false),
        )
    }

    /// Whether the close controls are revealed by the held modifier.
    ///
    /// Only the Ctrl group counts: Alt alone, and the Ctrl+Alt mix that AltGr
    /// reports, keep the close controls hidden just as they keep Ctrl hints off.
    fn revealing_tab_close(&self) -> bool {
        self.window.held_modifier == Some(ui::HeldModifier::Ctrl)
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
        self.tabs.active_id = id;
        self.pages.clear();
        self.toolbar.path.editing = false;
        if let Some(index) = self.tabs.items.iter().position(|slot| slot.id == id) {
            self.sidebar.regions.tabs_scroll.scroll_to_item(index);
        }
        self.sync_filter_input(window, cx);
        self.refresh_if_hidden_changed(cx);
        self.enter_location(cx);
        file_clipboard::refresh(cx);

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
            self.tabs.active_id = self.tabs.items[index.min(self.tabs.items.len() - 1)].id;
        }
        self.toolbar.path.editing = false;
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
        let work = IoTask::cancel_on_drop(self.window.tasks.spawn_cancellable(async move {
            let metadata =
                tokio::fs::metadata(&worker_path)
                    .await
                    .map_err(|source| desktop::Error::Io {
                        path: worker_path.clone(),
                        source,
                    })?;
            if metadata.is_dir() && mode == desktop::OpenMode::DefaultApplication {
                return Ok::<Activation, desktop::Error>(Activation::Directory);
            }
            desktop
                .open_file(&worker_path, mode)
                .await
                .map(Activation::File)
        }));
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
            .spawn_blocking(move || desktop.open_terminal(&path));
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
        if self.menus.open.is_some() && command != Command::Menu {
            self.menus.open = None;
            self.window.focus.focus(window, cx);
            if command == Command::Cancel {
                self.window.focus.focus(window, cx);
                cx.notify();
                return;
            }
        }
        if self.menus.context.is_some() && command != Command::ContextMenu {
            self.menus.context = None;
            self.window.focus.focus(window, cx);
            if command == Command::Cancel {
                cx.notify();
                return;
            }
        }
        if self.menus.sort.is_some() && command != Command::Sort {
            self.menus.sort = None;
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
                // Leaving the file workspace unmounts the menu, so take focus
                // back from it before its handle disappears.
                if self.menus.open.is_some() {
                    self.menus.open = None;
                    self.window.focus.focus(window, cx);
                }

                // The content region is about to unmount. Keep window actions
                // reachable until a settings control takes focus.
                self.show_page(pages::Page::Settings, window, cx);
                self.toolbar.path.editing = false;
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
                self.toolbar.path.editing = true;
                let path = self
                    .current_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.current_location().display_address());
                self.toolbar.path.input.update(cx, |input, cx| {
                    input.set_value(path, window, cx);
                    input.focus(window, cx);
                    input.select_all(window, cx);
                });
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

                    self.toolbar.path.editing = false;
                    self.pages.clear();
                    // Copy contents remain available after Escape.
                    file_clipboard::refresh(cx);
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
            error_kind = ?warning.error_kind, "{}", warning.message);
        let path = warning
            .path
            .as_os_str()
            .to_string_lossy()
            .escape_debug()
            .to_string();
        report = report.detail(format!("{path}: {}", warning.message));
    }
    report.replacing::<WarningNotice>().show(cx);
}

/// Report that preferences could not be applied or saved.
fn report_settings_failure(message_id: &str, error: impl std::fmt::Display, cx: &mut App) {
    let mut arguments = fluent_bundle::FluentArgs::new();
    arguments.set("error", error.to_string());
    notifications::Report::new(
        notifications::Level::Error,
        i18n::format(message_id, Some(&arguments), cx),
    )
    .replacing::<SettingsNotice>()
    .show(cx);
}
