use crate::entities::location::{Location, VolumeIdentity};
use gds::appearance::tokens;
mod breadcrumb;
mod clipboard_status;
mod columns;
mod context_menu;
#[cfg(target_os = "linux")]
mod desktop_requests;
mod devices;
mod drag_drop;
mod entry_appearance;
mod file_actions;
mod file_icon;
mod grid;
mod handlers;
mod history;
mod loading;
mod localization;
mod marquee;
mod menu;
mod new_folder;
mod picker;
mod place_pages;
mod preview;
mod recycle;
mod regions;
mod remote;
mod rename;
mod routing;
mod selection;
mod sidebar;
mod sort;
mod table;
mod tabs;
mod terminal;
mod transfers;
mod view;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::runtime::Handle;
use tokio_util::task::AbortOnDropHandle;

use crate::entities::location::volumes as storage;
use crate::entities::preferences as configuration;
use crate::features::file_operations::commands::*;
use crate::widgets::settings;
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
    Recent,
    Favorites,
    Network,
    Trash,
    Volumes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LocationTarget {
    Home,
    Recent,
    Favorites,
    Volumes,
    Network,
    Trash,
}

pub struct LaunchOptions {
    pub runtime: Handle,
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

pub struct Browser {
    connector: Entity<crate::features::connect_server::Connector>,
    window: AnyWindowHandle,
    remote_requests: std::collections::HashMap<u64, LoadRequest>,
    runtime: Handle,
    desktop: desktop::Desktop,
    root_focus: FocusHandle,
    focus: FocusHandle,
    regions: regions::State,
    path_input: Entity<InputState>,
    filter_input: Entity<InputState>,
    filter_sync_pending: bool,
    preview: Entity<preview::View>,
    thumbnails: Entity<ui::thumbnail::Cache>,
    rename: Entity<rename::State>,
    settings_view: Entity<settings::View>,
    theme_reload: Entity<gds::appearance::theme::HotReload>,
    file_table: Entity<gpui_kit::component::table::TableState<table::FileTable>>,
    marquee: marquee::State,
    column_editor: Entity<columns::Editor>,
    preferences: configuration::Preferences,
    places: Places,
    pinned_paths: Vec<PathBuf>,
    places_load_error: Option<String>,
    places_saving: bool,
    storage: Entity<storage::Service>,
    sidebar_split: Entity<ui::split::State>,
    pending_removals: Vec<storage::DeviceAction>,
    home: PathBuf,
    tabs: Vec<OpenTab>,
    volume_table: Entity<gpui_kit::component::table::TableState<devices::table::VolumeTable>>,
    volume_defaults: std::collections::HashMap<storage::VolumeId, PathBuf>,
    active_id: u64,
    next_id: u64,
    /// The tab row the pointer is over, so its close control follows hover as
    /// well as the held Ctrl group.
    hovered_tab: Option<u64>,
    /// State of an armed `Ctrl+Tab` prefix, if any.
    tab_prefix: Option<tabs::TabPrefix>,
    sidebar_collapsed: bool,
    /// The modifier group the window currently holds down, tracked so the tab
    /// close controls can be revealed and hidden again without a per-control
    /// timer.
    held_modifier: Option<ui::HeldModifier>,
    /// Whether the application menu is showing. The toolbar button and
    /// `Alt+M` / `F10` both drive this one flag.
    menu_open: bool,
    keyboard_menu: Option<context_menu::OpenMenu>,
    sort_menu: Option<sort::OpenMenu>,
    volume_menu_focus: Option<FocusHandle>,
    settings_open: bool,
    /// Sessions and selection belong to this window, never a file location.
    terminal: Option<terminal::Dock>,
    terminal_visible: bool,
    terminal_maximized: bool,
    terminal_split: Entity<ui::split::State>,
    terminal_close_pending: Option<terminal::CloseTarget>,
    terminal_closing: bool,
    path_editing: bool,
    file_operation_in_progress: bool,
    remote_paste_cancel: Option<tokio_util::sync::CancellationToken>,
    file_operation_paths: Vec<PathBuf>,
    /// The picker session, when this window is a file picker.
    picker: Option<picker::State>,
    recent_table: Entity<gpui_kit::component::table::TableState<place_pages::table::LocationTable>>,
    favorites_table:
        Entity<gpui_kit::component::table::TableState<place_pages::table::LocationTable>>,
    network_table:
        Entity<gpui_kit::component::table::TableState<place_pages::table::LocationTable>>,
    network: place_pages::Network,
    favorites_loading: bool,
    trash: recycle::State,
    _subscriptions: Vec<Subscription>,
}

impl Browser {
    pub fn new(options: LaunchOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let LaunchOptions {
            runtime,
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
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let path_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(i18n::text("path-placeholder", cx)));
        let filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(i18n::text("filter-placeholder", cx))
        });
        let settings_view = cx
            .new(|cx| settings::View::new(preferences.clone(), tokens::SIDEBAR_WIDTH, window, cx));
        let theme_reload =
            gds::appearance::theme::HotReload::shared(preferences.appearance.clone(), window, cx);
        let file_table = table::FileTable::make_state(cx.entity().downgrade(), window, cx);
        let volume_table =
            devices::table::VolumeTable::make_state(cx.entity().downgrade(), window, cx);
        let trash = recycle::State::new(cx.entity().downgrade(), window, cx);
        let recent_table = place_pages::table::LocationTable::make_state(
            cx.entity().downgrade(),
            Collection::Recent,
            window,
            cx,
        );
        let favorites_table = place_pages::table::LocationTable::make_state(
            cx.entity().downgrade(),
            Collection::Favorites,
            window,
            cx,
        );
        let network_table = place_pages::table::LocationTable::make_state(
            cx.entity().downgrade(),
            Collection::Network,
            window,
            cx,
        );
        let column_editor = cx.new(|_| columns::Editor::new());
        let thumbnails = cx.new(|_| ui::thumbnail::Cache::default());
        cx.observe(&thumbnails, |_, _, cx| cx.notify()).detach();
        let preview = cx.new(|cx| preview::View::new(thumbnails.clone(), cx));
        let rename = cx.new(|cx| rename::State::new(window, cx));
        let storage = storage::Service::shared(runtime.clone(), cx);
        let sidebar_split = cx.new(|cx| {
            ui::split::State::new(
                preferences.files.sidebar.tabs_height,
                sidebar::MIN_TABS_HEIGHT,
                sidebar::MIN_DEVICES_HEIGHT,
                cx,
            )
        });
        let terminal_split = cx.new(|cx| {
            let mut split = ui::split::State::new(
                preferences.files.terminal_content_height,
                terminal::MIN_CONTENT_HEIGHT,
                terminal::MIN_DOCK_HEIGHT,
                cx,
            );
            split.default_bottom(terminal::DEFAULT_DOCK_HEIGHT);
            split
        });
        Self::install_terminal_close_handler(window, cx);
        let subscriptions = vec![
            cx.observe(&terminal_split, |_, _, cx| cx.notify()),
            cx.subscribe(&terminal_split, Self::on_terminal_split),
            cx.subscribe_in(&path_input, window, Self::on_path_input),
            cx.subscribe_in(&filter_input, window, Self::on_filter_input),
            cx.subscribe_in(&settings_view, window, Self::on_settings_event),
            cx.observe(&storage, Self::on_storage_changed),
            cx.observe(&sidebar_split, |_, _, cx| cx.notify()),
            cx.subscribe(&sidebar_split, Self::on_sidebar_split),
            // Refresh both the footer contents and cut markers on activation.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    file_clipboard::refresh(cx);
                } else {
                    // An Alt release outside this window is never delivered
                    // here, so hint labels must not survive deactivation.
                    ui::keytips::release(window, cx);
                    this.cancel_marquee(cx);
                    drag_drop::cancel_tab_hover(cx);
                    // A release outside this window is never delivered, so an
                    // armed tab prefix must not survive into the next focus and
                    // commit a cycle the user never finished.
                    this.tab_prefix = None;
                    // The close controls follow the same press state, and a
                    // release outside this window would leave them showing.
                    if this.held_modifier.take().is_some() {
                        cx.notify();
                    }
                }
            }),
        ];
        let (places, state_error) = match places {
            Ok(places) => (places, None),
            Err(error) => (Places::default(), Some(error.to_string())),
        };
        if let Some(shutdown) = places.shutdown() {
            let runtime = runtime.clone();
            App::on_app_quit(cx, move |_| {
                let shutdown = shutdown.clone();
                let runtime = runtime.clone();
                async move {
                    if let Err(error) = runtime
                        .spawn(async move { shutdown.run().await })
                        .await
                        .map_err(crate::entities::location::places::Error::from)
                        .and_then(std::convert::identity)
                    {
                        tracing::error!(%error, "Cannot save Files state during shutdown");
                    }
                }
            })
            .detach();
        }
        let mut this = Self {
            connector: cx.new(|_| crate::features::connect_server::Connector::new(runtime.clone())),
            window: window.window_handle(),
            remote_requests: std::collections::HashMap::new(),
            runtime,
            desktop: desktop::Desktop::default(),
            root_focus: cx.focus_handle(),
            focus,
            regions: regions::State::new(cx),
            path_input,
            filter_input,
            filter_sync_pending: false,
            preview,
            thumbnails,
            rename,
            settings_view,
            theme_reload,
            file_table,
            marquee: marquee::State::default(),
            column_editor,
            preferences,
            pinned_paths: places.pinned.iter().map(PathBuf::from).collect(),
            places,
            places_load_error: state_error.clone(),
            places_saving: false,
            storage,
            sidebar_split,
            pending_removals: Vec::new(),
            home,
            tabs: Vec::new(),
            volume_table,
            volume_defaults: std::collections::HashMap::new(),
            active_id: 0,
            next_id: 1,
            hovered_tab: None,
            tab_prefix: None,
            sidebar_collapsed: false,
            held_modifier: None,
            menu_open: false,
            keyboard_menu: None,
            sort_menu: None,
            volume_menu_focus: None,
            settings_open: false,
            terminal: None,
            terminal_visible: false,
            terminal_maximized: false,
            terminal_split,
            terminal_close_pending: None,
            terminal_closing: false,
            path_editing: false,
            file_operation_in_progress: false,
            remote_paste_cancel: None,
            file_operation_paths: Vec::new(),
            picker: picker.map(|options| picker::State::new(options, window, cx)),
            recent_table,
            favorites_table,
            network_table,
            network: place_pages::Network::default(),
            favorites_loading: false,
            trash,
            _subscriptions: subscriptions,
        };
        this.install_remote_events(cx);
        if let Some(error) = startup_error {
            notifications::error(error, cx);
        }
        if let Some(error) = state_error {
            // Reported once; `save_places` stays disabled so an unreadable database
            // is never overwritten with empty state.
            notifications::error(error, cx);
        }
        let pinned: Vec<_> = this.places.pinned.iter().map(PathBuf::from).collect();
        for path in pinned {
            this.open_tab(path, true, window, cx);
        }
        if let Some(tab) = this
            .tabs
            .iter()
            .find(|tab| tab.path() == Some(start.as_path()))
        {
            this.active_id = tab.id;
        } else {
            this.open_tab(start, false, window, cx);
        }
        file_clipboard::refresh(cx);
        this
    }

    fn current_tab(&self) -> Option<&OpenTab> {
        self.tab_slot(self.active_id)
    }
    fn current_tab_mut(&mut self) -> Option<&mut OpenTab> {
        self.tab_slot_mut(self.active_id)
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
        self.tabs.iter().find(|slot| slot.id == id)
    }
    fn tab_slot_mut(&mut self, id: u64) -> Option<&mut OpenTab> {
        self.tabs.iter_mut().find(|slot| slot.id == id)
    }

    fn width(&self) -> f32 {
        if self.sidebar_collapsed {
            tokens::COLLAPSED_SIDEBAR_WIDTH
        } else {
            tokens::SIDEBAR_WIDTH
        }
    }

    fn current_location(&self) -> Location {
        self.current_tab()
            .map_or_else(|| Location::from(&self.home), OpenTab::location)
    }

    fn current_place_location(&self) -> Location {
        self.current_path()
            .as_ref()
            .map(Location::from)
            .unwrap_or_else(|| self.current_location())
    }

    fn details_visible(&self) -> bool {
        self.places.details_visible(self.current_location())
    }

    fn current_path(&self) -> Option<PathBuf> {
        self.active().and_then(OpenTab::path).map(Path::to_path_buf)
    }

    fn picker_projection(&self) -> Option<picker::Projection> {
        self.picker.as_ref().map(picker::State::projection)
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
        let id = self.next_id;
        self.next_id += 1;
        let mut slot = OpenTab::new(id, resolved.location, resolved.path, pinned);
        slot.device = resolved.device;
        slot.picker = self.picker_projection();
        self.tabs.push(slot);
        self.active_id = id;
        self.settings_open = false;
        self.path_editing = false;
        self.regions.tabs_scroll.scroll_to_item(self.tabs.len() - 1);
        if let Some(window) = window {
            self.sync_filter_input(window, cx);
            self.focus.focus(window, cx);
        }
        self.dismiss_tab_prefix();
        self.refresh_tab(id, cx);
    }

    fn start_request(&mut self, id: u64, request: LoadRequest, cx: &mut Context<Self>) {
        if let Some(slot) = self.tab_slot_mut(id) {
            slot.remote_load = None;
        }
        if let Location::Remote(address) = request.location() {
            self.load_remote(id, request.clone(), address.clone(), cx);
            return;
        }
        self.remote_requests.remove(&id);
        let expected = self.resolve_location(request.location(), cx).device;
        if request.path().is_none() {
            self.finish_request(id, &request, Ok(None), expected, cx);
            return;
        }
        let worker = request.clone();
        let work = cx.background_executor().spawn(async move { worker.load() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
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
                || device.is_current(&self.storage.read(cx).snapshot)
        });
        let Some(slot) = self.tabs.iter_mut().find(|slot| slot.id == id) else {
            return;
        };
        if !valid {
            if slot.navigation.cancel_request(request) {
                notifications::error(i18n::text("device-path-unavailable", cx), cx);
            }
            return;
        }
        match slot.navigation.finish_load(request, result) {
            Ok(LoadOutcome::Stale) => return,
            Ok(LoadOutcome::Applied) => {
                if !matches!(request.location(), Location::Remote(_)) {
                    slot.remote = None;
                }
                if let Some(directory) = slot.navigation.directory() {
                    report_directory_warnings(directory, cx);
                }
                slot.device = expected;
                slot.filter = slot.navigation.view().filter.clone();
                slot.anchor = None;
                slot.sort = columns::sort_for(&slot.location(), &self.places);
                slot.refresh_visible();
                slot.restore_scroll();
                let location = slot
                    .path()
                    .map(Location::from)
                    .unwrap_or_else(|| slot.location());
                self.places.record(location);
                self.save_places(cx);
            }
            Err(error) => report_load_failure(error.to_string(), cx),
        }
        if id == self.active_id {
            self.filter_sync_pending = true;
            self.enter_location(cx);
        }
        self.finish_pending_refresh(id, cx);
        cx.notify();
    }

    fn navigate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.navigate_tab(self.active_id, path, cx);
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
        if id == self.active_id {
            self.path_editing = false;
        }
        self.start_request(id, request, cx);
        file_clipboard::refresh(cx);
    }

    fn read_options(&self, id: u64) -> ReadOptions {
        ReadOptions {
            show_hidden: self.preferences.files.show_hidden
                || self.tab_slot(id).is_some_and(|slot| slot.reveal_hidden),
        }
    }

    fn on_path_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Blur) {
            if self.path_editing && !notifications::is_focused(window, cx) {
                self.path_editing = false;
                file_clipboard::refresh(cx);
                cx.notify();
            }
            return;
        }
        if !matches!(event, InputEvent::PressEnter { .. }) {
            return;
        }
        let value = input.read(cx).value();
        if ["location:", "file:", "sftp:", "smb:"]
            .iter()
            .any(|scheme| value.starts_with(scheme))
        {
            match Location::from_uri(&value) {
                Ok(Location::Directory(path)) => {
                    self.navigate(PathBuf::from(path), cx);
                    Self::restore_file_focus(cx.entity().downgrade(), self.active_id, window);
                }
                Ok(location) => self.show_location(location, window, cx),
                Err(error) => notifications::error(error.to_string(), cx),
            }
            return;
        }
        let current = self.current_path().unwrap_or_else(|| self.home.clone());
        let target = if value.as_ref() == current.to_string_lossy() {
            Ok(current.clone())
        } else {
            navigation::resolve_input(value.as_ref(), &current, Some(&self.home))
        };
        match target {
            Ok(path) => {
                self.navigate(path, cx);
                Self::restore_file_focus(cx.entity().downgrade(), self.active_id, window);
                file_clipboard::refresh(cx);
            }
            Err(error) => {
                notifications::Report::new(
                    notifications::Level::Error,
                    i18n::text("load-failed", cx),
                )
                .detail(error.to_string())
                .replacing::<LoadNotice>()
                .show(cx);
                cx.notify();
            }
        }
    }

    fn on_filter_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Blur) {
            file_clipboard::refresh(cx);
            return;
        }
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let value = input.read(cx).value().to_string();
        if let Some(slot) = self.active_mut() {
            slot.filter = value;
            slot.refresh_visible();
        }
        cx.notify();
    }

    fn sync_filter_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        let filter = self
            .active()
            .map(|tab| tab.filter.clone())
            .unwrap_or_default();
        if self.filter_input.read(cx).value().as_ref() != filter {
            self.filter_input
                .update(cx, |input, cx| input.set_value(filter, window, cx));
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
        if self.settings_open {
            return (None, None);
        }
        let ids: Vec<u64> = self.tabs.iter().map(|tab| tab.id).collect();
        (
            cycle_neighbour(&ids, self.active_id, true),
            cycle_neighbour(&ids, self.active_id, false),
        )
    }

    /// Whether the close controls are revealed by the held modifier.
    ///
    /// Only the Ctrl group counts: Alt alone, and the Ctrl+Alt mix that AltGr
    /// reports, keep the close controls hidden just as they keep Ctrl hints off.
    fn revealing_tab_close(&self) -> bool {
        self.held_modifier == Some(ui::HeldModifier::Ctrl)
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
        self.save_tab_view(self.active_id, cx);
        self.active_id = id;
        self.settings_open = false;
        self.path_editing = false;
        if let Some(index) = self.tabs.iter().position(|slot| slot.id == id) {
            self.regions.tabs_scroll.scroll_to_item(index);
        }
        self.sync_filter_input(window, cx);
        self.refresh_if_hidden_changed(cx);
        self.enter_location(cx);
        file_clipboard::refresh(cx);
        self.dismiss_tab_prefix();
        self.focus.focus(window, cx);
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
                    != (self.preferences.files.show_hidden
                        || self.active().is_some_and(|slot| slot.reveal_hidden))
            });
        if refresh_hidden {
            self.refresh_current(cx);
        }
    }

    fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tabs.iter().any(|tab| tab.id == id) {
            return;
        }
        self.close_tab_approved(id, window, cx);
    }

    fn close_tab_approved(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let was_active = self.active_id == id;
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.open_tab(self.home.clone(), false, window, cx);
        } else if was_active {
            self.active_id = self.tabs[index.min(self.tabs.len() - 1)].id;
        }
        self.path_editing = false;
        self.sync_filter_input(window, cx);
        self.focus.focus(window, cx);
        self.sync_pinned();
        self.save_places(cx);
        cx.notify();
    }

    fn toggle_pin(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(slot) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        slot.pinned = !slot.pinned;
        self.tabs.sort_by_key(|slot| !slot.pinned);
        self.sync_pinned();
        self.save_places(cx);
        cx.notify();
    }

    fn sync_pinned(&mut self) {
        let current: Vec<_> = self
            .tabs
            .iter()
            .filter(|tab| tab.pinned)
            .filter_map(|tab| tab.path().map(Path::to_path_buf))
            .collect();
        // Compare this window's tabs rather than the latest database snapshot,
        // which can contain pins added by another window.
        for path in &self.pinned_paths {
            if !current.contains(path) {
                self.places.set_pinned(path, false);
            }
        }
        for path in &current {
            if !self.pinned_paths.contains(path) {
                self.places.set_pinned(path, true);
            }
        }
        self.pinned_paths = current;
    }

    fn save_places(&mut self, cx: &mut Context<Self>) {
        if self.places_load_error.is_some() || self.places_saving {
            return;
        }
        let Some(save) = self.places.take_save() else {
            return;
        };
        self.places_saving = true;
        let work = self.runtime.spawn(async move { save.run().await });
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(crate::entities::location::places::Error::from)
                .and_then(std::convert::identity);
            let _ = this.update(cx, |browser, cx| {
                browser.places_saving = false;
                match result {
                    Ok(snapshot) => {
                        let views_changed = browser.places.views != snapshot.views
                            || browser.places.defaults != snapshot.defaults;
                        browser.places.accept(snapshot);
                        if views_changed {
                            browser.reproject_columns(cx);
                        }
                        browser.save_places(cx);
                    }
                    Err(error) => {
                        notifications::Report::new(notifications::Level::Error, error.to_string())
                            .replacing::<PlacesNotice>()
                            .show(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_favorite(&mut self, cx: &mut Context<Self>) {
        self.places.toggle_favorite(self.current_place_location());
        self.save_places(cx);
        cx.notify();
    }

    /// Toggle the bookmark of the visible location from the keyboard.
    ///
    /// `Ctrl+D` works in both directions: it adds the location when it is not
    /// bookmarked and removes it when it is, exactly like the star button.
    fn favorite_current_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open
            || self.current_tab().is_none_or(|slot| slot.is_loading())
            || self.editing_input_focused(window, cx)
        {
            return;
        }
        self.toggle_favorite(cx);
    }

    /// A path or filter edit owns its keys; workspace shortcuts stay out of it.
    fn editing_input_focused(&self, window: &Window, cx: &App) -> bool {
        self.editing_path(window, cx)
            || self
                .filter_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
    }

    /// Whether the path editor is open or holds focus.
    ///
    /// Its text lives only in the widget and losing focus discards the
    /// uncommitted path, so commands that would move focus stay out of it. The
    /// filter box commits on every change and keeps its word instead.
    fn editing_path(&self, window: &Window, cx: &App) -> bool {
        self.path_editing || self.path_input.read(cx).focus_handle(cx).is_focused(window)
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

        let target_tab = self.active_id;
        let worker_path = path.clone();
        let desktop = self.desktop.clone();
        let work = AbortOnDropHandle::new(self.runtime.spawn(async move {
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
                .map_err(|error| format!("无法完成打开任务：{error}"))
                .and_then(|result| result.map_err(|error| error.to_string()));
            if let Err(error) = this.update(cx, |this, cx| {
                match result {
                    Ok(Activation::Directory) => this.navigate_tab(target_tab, path, cx),
                    Ok(Activation::File(desktop::OpenOutcome::Opened)) => {
                        this.places.record(&path);
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
        let desktop = self.desktop.clone();
        let work = cx
            .background_executor()
            .spawn(async move { desktop.open_terminal(&path) });
        cx.spawn(async move |this, cx| {
            if let Err(error) = work.await {
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

    fn on_settings_event(
        &mut self,
        _: &Entity<settings::View>,
        event: &settings::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings::Event::Apply(preferences) = event;
        self.apply_preferences(preferences.clone(), window, cx);
    }

    fn apply_preferences(
        &mut self,
        preferences: configuration::Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if preferences == self.preferences {
            return;
        }
        let language_changed = preferences.language != self.preferences.language;
        let appearance_changed = preferences.appearance != self.preferences.appearance;
        let result = preferences.apply_shared(&self.preferences, cx);
        if let Err(error) = result {
            let previous = self.preferences.clone();
            self.settings_view
                .update(cx, |view, cx| view.set_preferences(previous, window, cx));
            report_settings_failure("settings-apply-failed", &error, cx);
            cx.notify();
            return;
        }
        let hidden_changed = preferences.files.show_hidden != self.preferences.files.show_hidden;
        self.preferences = preferences.clone();
        self.apply_terminal_preferences(cx);
        if appearance_changed {
            self.theme_reload.update(cx, |reload, cx| {
                reload.set_appearance(preferences.appearance.clone(), window, cx);
            });
        }
        if language_changed {
            // The shared language install succeeded; cached placeholder text and
            // the native window title are not read during render, so push them.
            self.sync_localized_fields(window, cx);
        }
        let save_error = configuration::save(&preferences).err();
        self.settings_view
            .update(cx, |view, cx| view.set_preferences(preferences, window, cx));
        if let Some(error) = save_error {
            report_settings_failure("settings-save-failed", &error, cx);
        }
        if hidden_changed {
            self.refresh_current(cx);
        }
        cx.notify();
    }

    fn refresh_current(&mut self, cx: &mut Context<Self>) {
        if self.collection().is_some() {
            return;
        }
        self.refresh_tab(self.active_id, cx);
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
        self.storage
            .update(cx, |service, cx| service.refresh_capacity(cx));
        let parents: std::collections::HashSet<_> =
            paths.iter().filter_map(|path| path.parent()).collect();
        let mut ready = Vec::new();
        for slot in &mut self.tabs {
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
        if self.rename.read(cx).is_open() || window.has_active_dialog(cx) {
            return;
        }
        if command == Command::Cancel
            && let Some(cancel) = &self.remote_paste_cancel
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
                    | Command::Terminal
                    | Command::Sidebar
                    | Command::Cancel
            )
        {
            return;
        }
        // Window actions can win their bindings before popup key listeners.
        // Close the controlled menu before handling its chosen command.
        if self.menu_open && command != Command::Menu {
            self.menu_open = false;
            if command == Command::Cancel {
                self.focus.focus(window, cx);
                cx.notify();
                return;
            }
        }
        if self.keyboard_menu.is_some() && command != Command::ContextMenu {
            self.keyboard_menu = None;
            self.focus.focus(window, cx);
            if command == Command::Cancel {
                cx.notify();
                return;
            }
        }
        if self.sort_menu.is_some() && command != Command::Sort {
            self.sort_menu = None;
            self.focus.focus(window, cx);
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
        if !self.settings_open
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
                if self.settings_open || self.editing_path(window, cx) {
                    return;
                }
                if self.menu_open {
                    // The menu is about to be unmounted; take focus back before
                    // its own handle disappears.
                    self.menu_open = false;
                    self.focus.focus(window, cx);
                } else {
                    self.menu_open = true;
                }
            }
            Command::Settings => {
                // Leaving the file workspace unmounts the menu, so take focus
                // back from it before its handle disappears.
                if self.menu_open {
                    self.menu_open = false;
                    self.focus.focus(window, cx);
                }
                self.reset_tab_prefix();
                // The content region is about to unmount. Keep window actions
                // reachable until a settings control takes focus.
                self.root_focus.focus(window, cx);
                self.settings_open = true;
                self.path_editing = false;
                self.settings_view.update(cx, |view, cx| {
                    view.set_preferences(self.preferences.clone(), window, cx)
                });
            }
            Command::NewTab => self.open_tab(self.home.clone(), false, window, cx),
            Command::CloseTab => {
                // Only an ordinary visible tab can claim Ctrl+W.
                if self.tabs.iter().any(|tab| tab.id == self.active_id) {
                    self.close_tab(self.active_id, window, cx);
                }
            }
            // Ctrl+Tab arms the file tab prefix while browsing; Settings keeps
            // cycling its own pages because the prefix belongs to the file
            // workspace context.
            Command::NextTab | Command::PreviousTab => {
                let backwards = matches!(command, Command::PreviousTab);
                if self.settings_open {
                    self.settings_view
                        .update(cx, |view, cx| view.cycle_page(backwards, window, cx));
                } else {
                    self.arm_tab_prefix(backwards, window, cx);
                }
            }
            Command::Sidebar => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
                let width = self.width();
                self.settings_view
                    .update(cx, |view, cx| view.set_sidebar_width(width, cx));
            }
            Command::Terminal => self.toggle_terminal(window, cx),
            Command::EditLocation => {
                self.path_editing = true;
                let path = self
                    .current_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.current_location().display_address());
                self.path_input.update(cx, |input, cx| {
                    input.set_value(path, window, cx);
                    input.focus(window, cx);
                    input.select_all(window, cx);
                });
            }
            Command::Back | Command::Forward | Command::Parent => {
                self.navigate_history(command, cx)
            }
            Command::Refresh => {
                self.storage.update(cx, |service, cx| service.refresh(cx));
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
                if !self.settings_open && self.collection().is_none() {
                    self.filter_input.update(cx, |input, cx| {
                        input.focus(window, cx);
                        input.select_all(window, cx);
                    });
                }
            }
            Command::Copy | Command::Cut => {
                if command == Command::Copy
                    && matches!(
                        self.collection(),
                        Some(Collection::Favorites | Collection::Network)
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
                if !self.settings_open
                    && self.collection().is_none()
                    && !window.has_active_dialog(cx)
                {
                    let visible = !self.details_visible();
                    self.places.set_details(self.current_location(), visible);
                    self.save_places(cx);
                    if !visible {
                        self.preview.update(cx, |preview, cx| preview.clear(cx));
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
            Command::Sort => self.set_sort_menu_open(self.sort_menu.is_none(), window, cx),
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
                    Some(Collection::Recent | Collection::Favorites | Collection::Network)
                ) && self.list_keys_available(window, cx)
                    && self.selected_place().is_some()
                {
                    self.favorite_selected_place(false, cx);
                } else {
                    self.favorite_current_location(window, cx);
                }
            }
            Command::Trash => {
                if self.collection() == Some(Collection::Favorites)
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
                if self.collection() == Some(Collection::Trash) {
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
                let mut preferences = self.preferences.clone();
                preferences.files.show_hidden = !preferences.files.show_hidden;
                self.apply_preferences(preferences, window, cx);
            }
            Command::Open => {
                if self.regions.sidebar_focused(window, cx) {
                    self.activate_region(window, cx);
                } else if self.collection() == Some(Collection::Volumes) {
                    if self.list_keys_available(window, cx)
                        && let Some(id) = self.volume_action_target(cx)
                    {
                        self.show_volume(id, window, cx);
                    }
                } else if matches!(
                    self.collection(),
                    Some(Collection::Recent | Collection::Favorites | Collection::Network)
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
                if self.connector.read(cx).connecting {
                    self.connector
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
                    if self.regions.sidebar_focused(window, cx) {
                        if !self.settings_open {
                            self.cancel_clipboard_cut(cx);
                        }
                        self.focus.focus(window, cx);
                        cx.notify();
                        return;
                    }
                    if self.cancel_marquee(cx) {
                        self.focus.focus(window, cx);
                        return;
                    }
                    // Esc leaves a location page where it is: only the
                    // transient modes (settings, path editing) are cancelled.
                    // A picker leaves the window only once those are closed,
                    // so the first Esc still closes its own local modes.
                    let local_mode = self.settings_open
                        || self.path_editing
                        || self.tab_prefix.is_some()
                        || notifications::is_focused(window, cx)
                        || self
                            .filter_input
                            .read(cx)
                            .focus_handle(cx)
                            .is_focused(window);
                    if !local_mode && self.cancel_clipboard_cut(cx) {
                        self.focus.focus(window, cx);
                        cx.notify();
                        return;
                    }
                    self.reset_tab_prefix();
                    self.path_editing = false;
                    self.settings_open = false;
                    // Copy contents remain available after Escape.
                    file_clipboard::refresh(cx);
                    self.focus.focus(window, cx);
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

#[cfg(test)]
mod tests;
