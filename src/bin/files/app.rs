mod breadcrumb;
mod columns;
mod context_menu;
mod devices;
mod drag_drop;
mod file_actions;
mod file_icon;
mod handlers;
mod localization;
mod marquee;
mod menu;
mod new_folder;
mod picker;
mod place_pages;
mod preview;
mod recycle;
mod regions;
mod rename;
mod selection;
mod sidebar;
mod table;
mod tabs;
mod terminal;
mod transfers;
mod view;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{actions::*, settings, storage};
use gds::{desktop, i18n, notifications, ui};
use gpui_kit::component::WindowExt;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::{
    directory::{self, ReadOptions},
    file_clipboard,
    navigation::{self, LoadOutcome, LoadRequest, Tab},
    places::Places,
    sorting,
};

#[derive(Clone, Copy, Eq, PartialEq)]
enum Collection {
    Recent,
    Favorites,
    Network,
    Trash,
    Volumes,
}

/// Id of the fixed Home page's own directory state.
///
/// Home never joins [`Browser::tabs`]: it keeps the launch home directory while
/// ordinary tabs come and go. Ordinary tab ids start at 1, so this id can never
/// collide with them.
const HOME_TAB_ID: u64 = 0;

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
    pub start: PathBuf,
    pub home: PathBuf,
    pub preferences: settings::Preferences,
    /// Errors gathered before the window existed, reported once as toasts.
    pub startup_error: Option<String>,
    /// Present when the window is a file picker instead of a browser.
    pub picker: Option<crate::picker::Options>,
}

/// Toast slots that keep repeated reports of one origin from stacking.
pub(super) struct LoadNotice;
pub(super) struct OpenNotice;
pub(super) struct WarningNotice;
pub(super) struct SettingsNotice;
pub(super) struct PlacesNotice;
pub(super) struct ClipboardNotice;
pub(super) struct TerminalNotice;
pub(super) struct RenameNotice;
pub(super) struct ColumnNotice;

/// Session cursor of one places list.
///
/// The cursor keeps the row's raw path, so two folders that print the same
/// stay distinct rows; it lives for this session and is never written to the
/// places file. Each page's table retains its own scroll position.
#[derive(Default)]
struct PlaceCursor {
    path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PageKind {
    FixedLocation,
    DirectoryTab,
}

struct OpenTab {
    id: u64,
    kind: PageKind,
    initial_path: PathBuf,
    initial_generation: u64,
    initial_loading: bool,
    needs_refresh: bool,
    tab: Option<Tab>,
    filter: String,
    visible: Arc<[usize]>,
    table_rows: Arc<[directory::Entry]>,
    scroll: UniformListScrollHandle,
    pending_selection: Option<PathBuf>,
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
    fn new(id: u64, path: PathBuf, pinned: bool) -> Self {
        Self {
            id,
            kind: PageKind::DirectoryTab,
            initial_path: path,
            initial_generation: 0,
            initial_loading: false,
            needs_refresh: false,
            tab: None,
            filter: String::new(),
            visible: Arc::from([]),
            table_rows: Arc::from([]),
            scroll: UniformListScrollHandle::new(),
            pending_selection: None,
            anchor: None,
            cursor: None,
            sort: sorting::Sort::default(),
            pinned,
            device: None,
            picker: None,
        }
    }

    fn fixed(id: u64, path: PathBuf) -> Self {
        Self {
            kind: PageKind::FixedLocation,
            ..Self::new(id, path, false)
        }
    }

    fn path(&self) -> &Path {
        self.tab
            .as_ref()
            .map_or(self.initial_path.as_path(), |tab| tab.navigation.path())
    }

    fn title(&self) -> String {
        self.path().file_name().map_or_else(
            || self.path().display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    fn is_loading(&self) -> bool {
        self.initial_loading
            || self
                .tab
                .as_ref()
                .is_some_and(|tab| tab.navigation.is_loading())
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
        let Some(tab) = &mut self.tab else {
            self.visible = Arc::from([]);
            self.table_rows = Arc::from([]);
            self.anchor = None;
            self.cursor = None;
            return;
        };
        let entries = &tab.navigation.directory().entries;
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
        tab.navigation
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
            tab.navigation.view_mut().selected = vec![path.clone()];
            self.cursor = Some(path.clone());
            self.anchor = Some(path.clone());
            if let Some(index) = self.table_rows.iter().position(|entry| entry.path == path) {
                self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
            }
        }
    }

    fn save_scroll(&mut self) {
        if let Some(tab) = &mut self.tab {
            tab.navigation.view_mut().scroll_offset =
                self.scroll.0.borrow().base_handle.offset().y.into();
        }
    }

    fn restore_scroll(&self) {
        if let Some(tab) = &self.tab {
            self.scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(px(0.), px(tab.navigation.view().scroll_offset)));
        }
    }
}

pub struct Browser {
    root_focus: FocusHandle,
    focus: FocusHandle,
    regions: regions::State,
    path_input: Entity<InputState>,
    filter_input: Entity<InputState>,
    preview: Entity<preview::View>,
    rename: Entity<rename::State>,
    settings_view: Entity<settings::View>,
    theme_reload: Entity<gds::theme::HotReload>,
    file_table: Entity<gpui_kit::component::table::TableState<table::FileTable>>,
    marquee: marquee::State,
    column_editor: Entity<columns::Editor>,
    preferences: settings::Preferences,
    places: Places,
    places_load_error: Option<String>,
    storage: Entity<storage::Service>,
    sidebar_split: Entity<ui::split::State>,
    pending_removals: Vec<storage::DeviceAction>,
    home: PathBuf,
    tabs: Vec<OpenTab>,
    /// The fixed Home page's own directory state; it never joins `tabs`.
    home_tab: OpenTab,
    volume_pages: devices::pages::Pages,
    volume_table: Entity<gpui_kit::component::table::TableState<devices::table::VolumeTable>>,
    volume_defaults: std::collections::HashMap<storage::VolumeId, PathBuf>,
    selected_volume: Option<storage::VolumeId>,
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
    preview_visible: bool,
    file_operation_in_progress: bool,
    file_operation_paths: Vec<PathBuf>,
    collection: Option<Collection>,
    /// The picker session, when this window is a file picker.
    picker: Option<picker::State>,
    /// Each places page retains its own keyboard cursor and table scroll.
    recent_cursor: PlaceCursor,
    favorites_cursor: PlaceCursor,
    network_cursor: PlaceCursor,
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
        let settings_view =
            cx.new(|cx| settings::View::new(preferences.clone(), ui::SIDEBAR_WIDTH, window, cx));
        let theme_reload =
            gds::theme::HotReload::shared(preferences.appearance.clone(), window, cx);
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
        let preview = cx.new(|_| preview::View::new());
        let rename = cx.new(|cx| rename::State::new(window, cx));
        let storage = storage::Service::shared(cx);
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
            // Another application may replace a cut after this window lost
            // focus, so revalidate the cached cut markers on activation. The
            // check is a no-op unless a cut is cached.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    file_clipboard::refresh_cut(cx);
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
        let (places, state_error) = match Places::load() {
            Ok(places) => (places, None),
            Err(error) => (Places::default(), Some(error.to_string())),
        };
        let home_tab = OpenTab::fixed(HOME_TAB_ID, home.clone());
        let mut this = Self {
            root_focus: cx.focus_handle(),
            focus,
            regions: regions::State::new(cx),
            path_input,
            filter_input,
            preview,
            rename,
            settings_view,
            theme_reload,
            file_table,
            marquee: marquee::State::default(),
            column_editor,
            preferences,
            places,
            places_load_error: state_error.clone(),
            storage,
            sidebar_split,
            pending_removals: Vec::new(),
            home,
            tabs: Vec::new(),
            home_tab,
            volume_pages: devices::pages::Pages::default(),
            volume_table,
            selected_volume: None,
            volume_defaults: std::collections::HashMap::new(),
            active_id: HOME_TAB_ID,
            next_id: 1,
            hovered_tab: None,
            tab_prefix: None,
            sidebar_collapsed: false,
            held_modifier: None,
            menu_open: false,
            keyboard_menu: None,
            volume_menu_focus: None,
            settings_open: false,
            terminal: None,
            terminal_visible: false,
            terminal_maximized: false,
            terminal_split,
            terminal_close_pending: None,
            terminal_closing: false,
            path_editing: false,
            preview_visible: false,
            file_operation_in_progress: false,
            file_operation_paths: Vec::new(),
            collection: None,
            picker: picker.map(picker::State::new),
            recent_cursor: PlaceCursor::default(),
            favorites_cursor: PlaceCursor::default(),
            network_cursor: PlaceCursor::default(),
            recent_table,
            favorites_table,
            network_table,
            network: place_pages::Network::default(),
            favorites_loading: false,
            trash,
            _subscriptions: subscriptions,
        };
        if let Some(error) = startup_error {
            notifications::error(error, cx);
        }
        if let Some(error) = state_error {
            // Reported once; `save_places` stays disabled so the unreadable file
            // is never overwritten by a fresh state.
            notifications::error(error, cx);
        }
        this.home_tab.picker = this.picker_projection();
        let pinned: Vec<_> = this.places.pinned.iter().map(PathBuf::from).collect();
        for path in pinned {
            this.open_tab(path, true, window, cx);
        }
        // The fixed Home page is the launch page when the requested folder is
        // the home directory; otherwise the requested folder opens as a tab.
        if start == this.home {
            this.show_home(window, cx);
        } else if let Some(tab) = this.tabs.iter().find(|tab| tab.path() == start) {
            this.active_id = tab.id;
        } else {
            this.open_tab(start, false, window, cx);
        }
        this
    }

    fn active(&self) -> Option<&OpenTab> {
        if self.collection.is_some() {
            return None;
        }
        self.tab_slot(self.active_id)
    }

    fn page_kind(&self) -> PageKind {
        // Virtual collections are fixed locations without a directory slot.
        self.active()
            .map_or(PageKind::FixedLocation, |slot| slot.kind)
    }

    fn active_mut(&mut self) -> Option<&mut OpenTab> {
        if self.collection.is_some() {
            return None;
        }
        self.tab_slot_mut(self.active_id)
    }

    /// One directory slot by id, including fixed Home and volume pages.
    fn tab_slot(&self, id: u64) -> Option<&OpenTab> {
        if id == HOME_TAB_ID {
            Some(&self.home_tab)
        } else {
            self.tabs
                .iter()
                .chain(self.volume_pages.entries.iter().map(|page| &page.slot))
                .find(|slot| slot.id == id)
        }
    }

    /// The mutable form of [`Self::tab_slot`].
    fn tab_slot_mut(&mut self, id: u64) -> Option<&mut OpenTab> {
        if id == HOME_TAB_ID {
            Some(&mut self.home_tab)
        } else {
            self.tabs
                .iter_mut()
                .chain(
                    self.volume_pages
                        .entries
                        .iter_mut()
                        .map(|page| &mut page.slot),
                )
                .find(|slot| slot.id == id)
        }
    }

    /// Whether the fixed Home page is the visible file page.
    fn home_active(&self) -> bool {
        self.collection.is_none() && self.active_id == HOME_TAB_ID
    }

    /// The session cursor of a places page, or `None` for a virtual location.
    fn place_cursor(&self, collection: Collection) -> Option<&PlaceCursor> {
        match collection {
            Collection::Recent => Some(&self.recent_cursor),
            Collection::Favorites => Some(&self.favorites_cursor),
            Collection::Network => Some(&self.network_cursor),
            Collection::Trash | Collection::Volumes => None,
        }
    }

    /// The mutable form of [`Self::place_cursor`].
    fn place_cursor_mut(&mut self, collection: Collection) -> Option<&mut PlaceCursor> {
        match collection {
            Collection::Recent => Some(&mut self.recent_cursor),
            Collection::Favorites => Some(&mut self.favorites_cursor),
            Collection::Network => Some(&mut self.network_cursor),
            Collection::Trash | Collection::Volumes => None,
        }
    }

    fn width(&self) -> f32 {
        if self.sidebar_collapsed {
            ui::COLLAPSED_SIDEBAR_WIDTH
        } else {
            ui::SIDEBAR_WIDTH
        }
    }

    fn current_path(&self) -> PathBuf {
        self.active()
            .map_or_else(|| self.home.clone(), |tab| tab.path().to_path_buf())
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

    /// Open an ordinary tab, optionally without a window to synchronize with.
    ///
    /// A path navigation started from the fixed Home page opens an ordinary
    /// tab instead of moving Home; that path has no window at hand, and the
    /// next render picks the filter value up from the tab itself.
    fn open_tab_inner(
        &mut self,
        path: PathBuf,
        pinned: bool,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        let mut slot = OpenTab::new(id, path, pinned);
        slot.picker = self.picker_projection();
        self.tabs.push(slot);
        self.regions.tabs_scroll.scroll_to_item(self.tabs.len() - 1);
        self.active_id = id;
        self.collection = None;
        self.settings_open = false;
        self.path_editing = false;
        if let Some(window) = window {
            self.sync_filter_input(window, cx);
            self.focus.focus(window, cx);
        }
        self.dismiss_tab_prefix();
        self.load_initial(id, cx);
    }

    fn load_initial(&mut self, id: u64, cx: &mut Context<Self>) {
        let options = ReadOptions {
            show_hidden: self.preferences.files.show_hidden,
        };
        let Some(slot) = slot_mut(
            id,
            &mut self.home_tab,
            &mut self.tabs,
            &mut self.volume_pages,
        ) else {
            return;
        };
        slot.initial_loading = true;
        slot.initial_generation += 1;
        let generation = slot.initial_generation;
        let path = slot.initial_path.clone();
        let expected_device =
            devices::DeviceLocation::capture(&self.storage.read(cx).snapshot, &path);
        let work = cx
            .background_executor()
            .spawn(async move { directory::read_directory(&path, options) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            if let Err(error) = this.update(cx, |this, cx| {
                let storage_snapshot = &this.storage.read(cx).snapshot;
                let device_valid = expected_device
                    .as_ref()
                    .is_none_or(|device| device.is_current(storage_snapshot));
                let current_device = result.as_ref().ok().and_then(|snapshot| {
                    devices::DeviceLocation::capture(storage_snapshot, &snapshot.path)
                });
                let Some(slot) = slot_mut(
                    id,
                    &mut this.home_tab,
                    &mut this.tabs,
                    &mut this.volume_pages,
                ) else {
                    return;
                };
                if slot.initial_generation != generation {
                    return;
                }
                slot.initial_loading = false;
                if !device_valid {
                    slot.device = expected_device;
                    if let Some(device) = &mut slot.device {
                        device.update(storage_snapshot);
                    }
                    slot.refresh_visible();
                    notifications::error(i18n::text("device-path-unavailable", cx), cx);
                    cx.notify();
                    return;
                }
                match result {
                    Ok(snapshot) => {
                        report_directory_warnings(&snapshot, cx);
                        let path = snapshot.path.clone();
                        let mut tab = Tab::new(snapshot);
                        tab.pinned = slot.pinned;
                        slot.tab = Some(tab);
                        slot.sort = columns::sort_for(
                            &path,
                            slot.sort,
                            &this.places,
                            &this.preferences.files.columns,
                        );
                        slot.device = current_device;
                        slot.refresh_visible();
                        this.places.record(&path);
                        this.save_places(cx);
                    }
                    Err(error) => {
                        report_load_failure(error.to_string(), cx);
                    }
                }
                this.finish_pending_refresh(id, cx);
                cx.notify();
            }) {
                eprintln!("Directory result arrived after the view closed: {error}");
            }
        })
        .detach();
        cx.notify();
    }

    fn start_request(&mut self, id: u64, request: LoadRequest, cx: &mut Context<Self>) {
        let expected_device =
            devices::DeviceLocation::capture(&self.storage.read(cx).snapshot, request.path());
        if expected_device.is_none() && !self.device_path_available(request.path(), cx) {
            if let Some(tab) = self.tab_slot_mut(id).and_then(|slot| slot.tab.as_mut()) {
                tab.navigation.cancel_request(&request);
            }
            notifications::error(i18n::text("device-path-unavailable", cx), cx);
            cx.notify();
            return;
        }
        let worker_request = request.clone();
        let work = cx
            .background_executor()
            .spawn(async move { worker_request.load() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            if let Err(error) = this.update(cx, |this, cx| {
                let storage_snapshot = &this.storage.read(cx).snapshot;
                let device_valid = expected_device
                    .as_ref()
                    .is_none_or(|device| device.is_current(storage_snapshot));
                let current_device = result.as_ref().ok().and_then(|snapshot| {
                    devices::DeviceLocation::capture(storage_snapshot, &snapshot.path)
                });
                let Some(slot) = slot_mut(
                    id,
                    &mut this.home_tab,
                    &mut this.tabs,
                    &mut this.volume_pages,
                ) else {
                    return;
                };
                let Some(tab) = &mut slot.tab else {
                    return;
                };
                if !device_valid {
                    if tab.navigation.cancel_request(&request) {
                        notifications::error(i18n::text("device-path-unavailable", cx), cx);
                        cx.notify();
                    }
                    return;
                }
                match tab.navigation.finish_load(&request, result) {
                    Ok(LoadOutcome::Stale) => return,
                    Ok(LoadOutcome::Applied) => {
                        let path = tab.navigation.path().to_path_buf();
                        report_directory_warnings(tab.navigation.directory(), cx);
                        slot.anchor = None;
                        slot.sort = columns::sort_for(
                            &path,
                            slot.sort,
                            &this.places,
                            &this.preferences.files.columns,
                        );
                        // Reconnecting only releases the unavailable screen
                        // after the requested relative directory was read.
                        slot.device = current_device;
                        slot.refresh_visible();
                        slot.restore_scroll();
                        this.places.record(&path);
                        this.save_places(cx);
                    }
                    Err(error) => {
                        report_load_failure(error.to_string(), cx);
                    }
                }
                this.finish_pending_refresh(id, cx);
                cx.notify();
            }) {
                eprintln!("Navigation result arrived after the view closed: {error}");
            }
        })
        .detach();
        cx.notify();
    }

    fn navigate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.collection.is_some() {
            self.open_tab_inner(path, false, None, cx);
            return;
        }
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
        if !explicit_device
            && let Some(slot) = self.tab_slot(id)
            && slot.kind == PageKind::FixedLocation
        {
            let activate = self.active_id == id && self.collection.is_none() && !self.settings_open;
            if slot.path() != path {
                let previous = (
                    self.active_id,
                    self.collection,
                    self.settings_open,
                    self.path_editing,
                );
                self.open_tab_inner(path, false, None, cx);
                // A delayed directory activation must not steal the page the
                // user switched to while its metadata was being read.
                if !activate {
                    (
                        self.active_id,
                        self.collection,
                        self.settings_open,
                        self.path_editing,
                    ) = previous;
                }
            } else if activate {
                self.path_editing = false;
                cx.notify();
            }
            return;
        }
        if self.active_id == id {
            self.collection = None;
            self.path_editing = false;
        }
        let Some(slot) = self.tab_slot_mut(id) else {
            return;
        };
        slot.save_scroll();
        let id = slot.id;
        if let Some(tab) = &mut slot.tab {
            let request = tab.navigation.prepare_open(path);
            self.start_request(id, request, cx);
        } else {
            slot.initial_path = path;
            self.load_initial(id, cx);
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
                file_clipboard::refresh_cut(cx);
                cx.notify();
            }
            return;
        }
        if !matches!(event, InputEvent::PressEnter { .. }) {
            return;
        }
        let value = input.read(cx).value();
        let current = self.current_path();
        let target = if value.as_ref() == current.to_string_lossy() {
            Ok(current.clone())
        } else {
            navigation::resolve_input(value.as_ref(), &current, Some(&self.home))
        };
        match target {
            Ok(path) => {
                self.navigate(path, cx);
                Self::restore_file_focus(cx.entity().downgrade(), self.active_id, window);
                file_clipboard::refresh_cut(cx);
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
        if !ids.iter().any(|id| *id == self.active_id) {
            // Home and the collection pages are not part of the ordinary strip;
            // the first press enters it from either end.
            return (ids.last().copied(), ids.first().copied());
        }
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
        if id == HOME_TAB_ID {
            self.show_home(window, cx);
            return;
        }
        if let Some(slot) = self.active_mut() {
            slot.save_scroll();
        }
        self.active_id = id;
        self.collection = None;
        self.path_editing = false;
        if let Some(index) = self.tabs.iter().position(|slot| slot.id == id) {
            self.regions.tabs_scroll.scroll_to_item(index);
        }
        self.sync_filter_input(window, cx);
        self.refresh_if_hidden_changed(cx);
        self.dismiss_tab_prefix();
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Refresh the visible folder when it was read with another hidden-file
    /// setting than the current preference.
    fn refresh_if_hidden_changed(&mut self, cx: &mut Context<Self>) {
        let refresh_hidden = self
            .active()
            .and_then(|slot| slot.tab.as_ref())
            .is_some_and(|tab| {
                tab.navigation.directory().options.show_hidden != self.preferences.files.show_hidden
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
        let was_active = self.collection.is_none() && self.active_id == id;
        self.tabs.remove(index);
        if was_active && self.tabs.is_empty() {
            // The last ordinary tab closes into the fixed Home page instead of
            // creating another ordinary tab for the same folder.
            self.show_home(window, cx);
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
        if let Some(slot) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            slot.pinned = !slot.pinned;
            if let Some(tab) = &mut slot.tab {
                tab.pinned = slot.pinned;
            }
        }
        self.tabs.sort_by_key(|slot| !slot.pinned);
        self.sync_pinned();
        self.save_places(cx);
        cx.notify();
    }

    fn sync_pinned(&mut self) {
        self.places.pinned = self
            .tabs
            .iter()
            .filter(|tab| tab.pinned)
            .map(|tab| tab.path().as_os_str().to_owned())
            .collect();
    }

    fn save_places(&mut self, cx: &mut Context<Self>) {
        if self.places_load_error.is_some() {
            // Reported once at startup; saving would overwrite the unreadable
            // file with a fresh, lossy state.
            return;
        }
        if let Err(error) = self.places.save() {
            notifications::Report::new(notifications::Level::Error, error.to_string())
                .replacing::<PlacesNotice>()
                .show(cx);
        }
    }

    fn toggle_favorite(&mut self, cx: &mut Context<Self>) {
        let path = self.current_path();
        self.places.toggle_favorite(&path);
        self.save_places(cx);
        cx.notify();
    }

    /// Toggle the bookmark of the visible folder from the keyboard.
    ///
    /// `Ctrl+D` works in both directions: it adds the folder when it is not
    /// bookmarked and removes it when it is, exactly like the star button.
    fn favorite_current_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.folder_settled() || self.editing_input_focused(window, cx) {
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
        let source_kind = self.page_kind();
        let worker_path = path.clone();
        let work = cx.background_executor().spawn(async move {
            let metadata = std::fs::metadata(&worker_path)?;
            if metadata.is_dir() && mode == desktop::OpenMode::DefaultApplication {
                return Ok::<Activation, anyhow::Error>(Activation::Directory);
            }
            desktop::open_file(&worker_path, mode)
                .await
                .map(Activation::File)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            if let Err(error) = this.update(cx, |this, cx| {
                match result {
                    Ok(Activation::Directory) if source_kind == PageKind::FixedLocation => {
                        this.open_tab_inner(path, false, None, cx);
                    }
                    Ok(Activation::Directory) => this.navigate_tab(target_tab, path, cx),
                    Ok(Activation::File(desktop::OpenOutcome::Opened)) => {
                        this.places.record(&path);
                        this.save_places(cx);
                    }
                    Ok(Activation::File(desktop::OpenOutcome::Cancelled)) => {}
                    Err(error) => notifications::Report::new(
                        notifications::Level::Error,
                        format!("{error:#}"),
                    )
                    .replacing::<OpenNotice>()
                    .show(cx),
                }
                cx.notify();
            }) {
                eprintln!("Open result arrived after the view closed: {error}");
            }
        })
        .detach();
    }

    fn open_external_terminal(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.device_path_available(&path, cx) {
            return;
        }
        let work = cx
            .background_executor()
            .spawn(async move { desktop::open_terminal(&path) });
        cx.spawn(async move |this, cx| {
            if let Err(error) = work.await {
                if let Err(closed) = this.update(cx, |_, cx| {
                    notifications::Report::new(notifications::Level::Error, error.to_string())
                        .replacing::<TerminalNotice>()
                        .show(cx);
                    cx.notify();
                }) {
                    eprintln!("Terminal launch failed after the view closed: {error}; {closed}");
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
        preferences: settings::Preferences,
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
        let save_error = settings::save(&preferences).err();
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
        if self.collection.is_some() {
            return;
        }
        self.refresh_tab(self.active_id, cx);
    }

    fn refresh_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        if self
            .tab_slot(id)
            .is_some_and(|slot| !slot.files_available())
        {
            return;
        }
        let options = ReadOptions {
            show_hidden: self.preferences.files.show_hidden,
        };
        let Some(slot) = slot_mut(
            id,
            &mut self.home_tab,
            &mut self.tabs,
            &mut self.volume_pages,
        ) else {
            return;
        };
        slot.save_scroll();
        if let Some(tab) = &mut slot.tab {
            let request = tab.navigation.prepare_refresh(options);
            self.start_request(id, request, cx);
        } else {
            self.load_initial(id, cx);
        }
    }

    fn finish_pending_refresh(&mut self, id: u64, cx: &mut Context<Self>) {
        let refresh = slot_mut(
            id,
            &mut self.home_tab,
            &mut self.tabs,
            &mut self.volume_pages,
        )
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
        for slot in std::iter::once(&mut self.home_tab)
            .chain(self.tabs.iter_mut())
            .chain(
                self.volume_pages
                    .entries
                    .iter_mut()
                    .map(|page| &mut page.slot),
            )
        {
            let affected = parents.contains(slot.path())
                || paths.iter().any(|path| slot.path().starts_with(path));
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
                if self.collection.is_none() && self.tabs.iter().any(|tab| tab.id == self.active_id)
                {
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
                // The collection pages are virtual locations; there is no
                // editable path behind them.
                if self.collection.is_some() {
                    return;
                }
                self.path_editing = true;
                let path = self.current_path().to_string_lossy().into_owned();
                self.path_input.update(cx, |input, cx| {
                    input.set_value(path, window, cx);
                    input.focus(window, cx);
                    input.select_all(window, cx);
                });
            }
            Command::Back | Command::Forward | Command::Parent => {
                // The collection pages have no navigation history or path of
                // their own.
                if self.collection.is_some() {
                    return;
                }
                self.path_editing = false;
                if self.active_id == HOME_TAB_ID {
                    // Home stays fixed at the home directory; moving out of it
                    // starts an ordinary tab instead.
                    if matches!(command, Command::Parent)
                        && let Some(parent) = self.home.parent().map(Path::to_path_buf)
                    {
                        self.navigate(parent, cx);
                    }
                    return;
                }
                if self.volume_pages.active(self.active_id).is_some() {
                    if matches!(command, Command::Parent)
                        && let Some(parent) = self.current_path().parent().map(Path::to_path_buf)
                    {
                        self.navigate(parent, cx);
                    }
                    return;
                }
                if let Some(slot) = self.active_mut() {
                    slot.save_scroll();
                    let id = slot.id;
                    let request = slot.tab.as_mut().and_then(|tab| match command {
                        Command::Back => tab.navigation.prepare_back(),
                        Command::Forward => tab.navigation.prepare_forward(),
                        _ => tab.navigation.prepare_parent(),
                    });
                    if let Some(request) = request {
                        self.start_request(id, request, cx);
                    }
                }
            }
            Command::Refresh => {
                self.storage.update(cx, |service, cx| service.refresh(cx));
                if self.collection == Some(Collection::Trash) {
                    self.refresh_trash(cx);
                } else if self.collection == Some(Collection::Network) {
                    self.refresh_network(cx);
                } else if self.collection == Some(Collection::Favorites) {
                    self.refresh_favorites(cx);
                } else {
                    self.refresh_current(cx);
                }
            }
            Command::Filter => {
                if !self.settings_open && self.collection.is_none() {
                    self.filter_input.update(cx, |input, cx| {
                        input.focus(window, cx);
                        input.select_all(window, cx);
                    });
                }
            }
            Command::Copy | Command::Cut => {
                if command == Command::Copy
                    && matches!(
                        self.collection,
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
                    self.paste_files(self.current_path(), window, cx);
                }
            }
            Command::CopyPaths => {
                if self.list_keys_available(window, cx)
                    && let Some(path) = self.selected_place_path()
                {
                    self.copy_selected_paths(&[path], cx);
                } else if self.file_selection_focused(window, cx) {
                    let paths = self.selected_visible_paths();
                    if !paths.is_empty() {
                        self.copy_selected_paths(&paths, cx);
                    }
                }
            }
            Command::OpenInNewTab => {
                if self.list_keys_available(window, cx)
                    && let Some(path) = self.selected_place_path()
                {
                    self.open_tab(path, false, window, cx);
                } else if self.file_selection_focused(window, cx) {
                    self.open_selected_in_new_tabs(window, cx);
                }
            }
            Command::SelectAll => {
                if self.collection == Some(Collection::Trash) {
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
                if self.collection == Some(Collection::Volumes) {
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
                if !self.settings_open && self.collection.is_none() && !window.has_active_dialog(cx)
                {
                    self.preview_visible = !self.preview_visible;
                    if !self.preview_visible {
                        self.preview.update(cx, |preview, cx| preview.clear(cx));
                    }
                }
            }
            Command::Columns => {
                if !self.editing_input_focused(window, cx) && self.collection.is_none() {
                    self.open_columns_editor(window, cx);
                }
            }
            Command::Favorite => {
                if matches!(
                    self.collection,
                    Some(Collection::Favorites | Collection::Network)
                ) && self.list_keys_available(window, cx)
                {
                    self.favorite_selected_place(false, cx);
                } else {
                    self.favorite_current_folder(window, cx);
                }
            }
            Command::Trash => {
                if self.collection == Some(Collection::Favorites)
                    && self.list_keys_available(window, cx)
                {
                    self.favorite_selected_place(true, cx);
                } else if self.collection == Some(Collection::Trash) {
                    // Delete on a trashed record is a permanent deletion, not a
                    // second move into Trash; it keeps the confirmation step.
                    self.request_trash_purge(window, cx);
                } else if self.file_selection_focused(window, cx) {
                    self.request_trash_selection(window, cx);
                }
            }
            Command::DeletePermanent => {
                if self.collection == Some(Collection::Trash) {
                    self.request_trash_purge(window, cx);
                } else if self.file_selection_focused(window, cx) {
                    self.request_delete_selection(window, cx);
                }
            }
            Command::RestoreSelection => {
                if self.collection == Some(Collection::Trash) {
                    self.restore_selected_trash(cx);
                }
            }
            Command::PurgeSelection => {
                if self.collection == Some(Collection::Trash) {
                    self.request_trash_purge(window, cx);
                }
            }
            Command::EmptyTrash => {
                if self.collection == Some(Collection::Trash) {
                    self.request_empty_trash(window, cx);
                }
            }
            Command::ClearRecent => {
                if self.collection == Some(Collection::Recent) {
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
                } else if self.collection == Some(Collection::Volumes) {
                    if self.list_keys_available(window, cx)
                        && let Some(id) = self.volume_action_target(cx)
                    {
                        self.open_volume(id, window, cx);
                    }
                } else if matches!(
                    self.collection,
                    Some(Collection::Recent | Collection::Favorites | Collection::Network)
                ) {
                    // A places page opens its keyboard selection; the mouse
                    // keeps opening a row directly.
                    self.open_place_cursor(window, cx);
                } else if self.file_selection_focused(window, cx)
                    && self.active().is_some_and(|slot| !slot.is_loading())
                    && let Some(path) = self
                        .active()
                        .and_then(|slot| {
                            slot.cursor.as_ref().or_else(|| {
                                slot.tab
                                    .as_ref()
                                    .and_then(|tab| tab.navigation.view().selected.first())
                            })
                        })
                        .cloned()
                {
                    self.activate_path(path, cx);
                }
            }
            Command::Cancel => {
                if self.regions.sidebar_focused(window, cx) {
                    self.focus.focus(window, cx);
                    cx.notify();
                    return;
                }
                // GPUI dispatches bound actions before key listeners and stops
                // propagation on the action, so `on_file_key_down` never sees
                // the escape that cancels a pointer drag. Cancelling a drag ends
                // only the drag: the release must not transfer files and the
                // selection stays intact.
                drag_drop::cancel_tab_hover(cx);
                if !cx.stop_active_drag(window) {
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
                        || self.picker.is_some()
                            && self
                                .filter_input
                                .read(cx)
                                .focus_handle(cx)
                                .is_focused(window);
                    self.reset_tab_prefix();
                    self.path_editing = false;
                    self.settings_open = false;
                    // Esc is a natural moment to revalidate a cached cut that
                    // another application may have replaced while this view was
                    // elsewhere.
                    file_clipboard::refresh_cut(cx);
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

/// One directory slot by id, including fixed Home and volume pages.
fn slot_mut<'a>(
    id: u64,
    home: &'a mut OpenTab,
    tabs: &'a mut Vec<OpenTab>,
    volumes: &'a mut devices::pages::Pages,
) -> Option<&'a mut OpenTab> {
    if id == HOME_TAB_ID {
        Some(home)
    } else {
        tabs.iter_mut()
            .chain(volumes.entries.iter_mut().map(|page| &mut page.slot))
            .find(|slot| slot.id == id)
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
