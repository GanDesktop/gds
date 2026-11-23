//! A window-owned workspace. Services finish on Tokio; `zgui` owners receive results.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    ffi::OsString,
    future::Future,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
};

use gds::{
    app::Gds,
    appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH},
    service::task_manager::{Owner as TaskOwner, Scope},
    ui::sidebar::{SidebarPresentation, SidebarState, use_sidebar_presentation},
};
use tokio_util::sync::CancellationToken;
use zgui::prelude::*;
use zgui::reactive::RenderEffect;

use super::{
    model::{
        AddressPresentation, BackgroundTask, BackgroundTasks, ConfirmOptions, Contents,
        FILES_TASK_KEY, Operation, Pages, Pane, Prompt, State, Tab,
    },
    name_form::{NameForm, NamePurpose, PromptDecisionSlot},
};
use crate::{
    entities::{
        file::columns::Column,
        location::{
            Location,
            places::{Error as PlacesError, Places},
            volumes,
        },
        preferences,
    },
    features::pick_files,
    shared::lib::clipboard::Clipboard,
};

pub(super) const DEFAULT_WINDOW_WIDTH: f32 = gds::ui::window::INITIAL_WIDTH;
pub(super) const DEFAULT_WINDOW_HEIGHT: f32 = gds::ui::window::INITIAL_HEIGHT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FilterFocusMode {
    SelectAll,
    CaretEnd,
}

pub(super) fn resolve_initial_widths(reported_width: Option<f32>) -> (f32, f32) {
    let window_width = reported_width
        .filter(|width| width.is_finite() && *width > 0.0)
        .unwrap_or(DEFAULT_WINDOW_WIDTH);
    let sidebar_width = if gds::ui::window::WidthTier::from_width(window_width)
        == gds::ui::window::WidthTier::Micro
    {
        COLLAPSED_SIDEBAR_WIDTH
    } else {
        SIDEBAR_WIDTH
    };
    (window_width, (window_width - sidebar_width).max(1.0))
}

pub(super) fn resolve_initial_height(reported_height: Option<f32>) -> f32 {
    reported_height
        .filter(|height| height.is_finite() && *height > 0.0)
        .unwrap_or(DEFAULT_WINDOW_HEIGHT)
}

pub(super) fn sample_workspace_sidebar(
    sidebar: RwSignal<bool, LocalStorage>,
    window_width: RwSignal<f32, LocalStorage>,
) -> (
    Signal<SidebarPresentation, LocalStorage>,
    Signal<f32, LocalStorage>,
) {
    let sidebar_state = Signal::derive_local(move || {
        SidebarState::from_collapsed(
            !sidebar.get()
                || gds::ui::window::WidthTier::from_width(window_width.get())
                    == gds::ui::window::WidthTier::Micro,
        )
    });
    let presentation = use_sidebar_presentation(sidebar_state);
    let width = Signal::derive_local(move || presentation.get().width());
    (presentation, width)
}

pub(super) fn observe_window_title(
    prefs: ReadSignal<preferences::Preferences, LocalStorage>,
    ready: impl Fn() -> bool + 'static,
    mut apply_title: impl FnMut(&str) + 'static,
) {
    let title_fx = RenderEffect::new(move |prev: Option<Option<String>>| {
        let _ = prefs.with(|p| p.language);
        let _ = Gds::current().i18n().language();
        if !ready() {
            return None;
        }
        let title = gds::i18n::text("app-title");
        if prev.flatten().as_deref() != Some(title.as_str()) {
            apply_title(&title);
        }
        Some(title)
    });
    on_cleanup_local(move || drop(title_fx));
}

pub(crate) struct LaunchOptions {
    pub places: Result<Places, PlacesError>,
    pub start: PathBuf,
    pub home: PathBuf,
    pub picker: Option<pick_files::Options>,
}

#[derive(Clone)]
pub(crate) struct Browser(pub(super) Rc<Workspace>);

pub(crate) struct WeakBrowser(Weak<Workspace>);

impl Clone for WeakBrowser {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl WeakBrowser {
    pub(crate) fn upgrade(&self) -> Option<Browser> {
        self.0
            .upgrade()
            .map(Browser)
            .filter(|browser| browser.0.alive.get())
    }
}

#[derive(Clone, Copy)]
pub(super) struct DragSources(RwSignal<HashSet<PathBuf>, LocalStorage>);

impl DragSources {
    pub fn new() -> Self {
        Self(RwSignal::new_local(HashSet::new()))
    }

    pub fn read_only(self) -> ReadSignal<HashSet<PathBuf>, LocalStorage> {
        self.0.read_only()
    }

    pub fn set(self, paths: impl IntoIterator<Item = PathBuf>) {
        let next: HashSet<PathBuf> = paths.into_iter().collect();
        if self.0.with_untracked(|current| *current != next) {
            self.0.set(next);
        }
    }

    pub fn clear(self) {
        if !self.0.with_untracked(HashSet::is_empty) {
            self.0.set(HashSet::new());
        }
    }

    pub fn contains(self, path: &Path) -> bool {
        self.0
            .with(|sources| !sources.is_empty() && sources.contains(path))
    }

    #[cfg(test)]
    pub fn contains_untracked(self, path: &Path) -> bool {
        self.0
            .with_untracked(|sources| !sources.is_empty() && sources.contains(path))
    }

    pub fn is_empty_untracked(self) -> bool {
        self.0.with_untracked(HashSet::is_empty)
    }
}

pub(super) struct Workspace {
    pub alive: Cell<bool>,
    pub state: RefCell<State>,
    pub revision: RwSignal<u64, LocalStorage>,
    pub selection: RwSignal<u64, LocalStorage>,
    pub column_widths: RwSignal<HashMap<Column, f32>, LocalStorage>,
    pub pages: RwSignal<Pages, LocalStorage>,
    pub window_width: RwSignal<f32, LocalStorage>,
    pub window_height: RwSignal<f32, LocalStorage>,
    pub content_width: RwSignal<f32, LocalStorage>,
    pub footer_filter_open: RwSignal<bool, LocalStorage>,
    pub filter_focus_request: RwSignal<Option<FilterFocusMode>, LocalStorage>,
    pub micro_locations_expanded: RwSignal<bool, LocalStorage>,
    pub address_presentation: RwSignal<AddressPresentation, LocalStorage>,
    pub sidebar_tabs_height: RwSignal<Option<f32>, LocalStorage>,
    pub settings: crate::widgets::settings::View,
    pub sidebar: RwSignal<bool, LocalStorage>,
    pub sidebar_presentation: Signal<SidebarPresentation, LocalStorage>,
    pub sidebar_width: Signal<f32, LocalStorage>,
    pub details: RwSignal<bool, LocalStorage>,
    pub terminal: RwSignal<bool, LocalStorage>,
    pub background_tasks: RwSignal<BackgroundTasks, LocalStorage>,
    pub background_tasks_revision: RwSignal<u64, LocalStorage>,
    pub drag_sources: DragSources,
    pub notice: RwSignal<Option<String>, LocalStorage>,
    pub prompt: RwSignal<Prompt, LocalStorage>,
    pub prompt_generation: RwSignal<u64, LocalStorage>,
    pub confirm_options: RwSignal<ConfirmOptions, LocalStorage>,
    pub prompt_decisions: PromptDecisionSlot,
    pub address: RwSignal<String, LocalStorage>,
    pub filter: RwSignal<String, LocalStorage>,
    pub save_name: RwSignal<String, LocalStorage>,
    pub list: NodeRef,
    pub content_focus: NodeRef,
    pub root_focus: NodeRef,
    pub list_focused: Cell<bool>,
    pub list_focus_visible: RwSignal<bool, LocalStorage>,
    pub settings_node: NodeRef,
    pub address_node: NodeRef,
    pub filter_node: NodeRef,
    pub tabs_node: NodeRef,
    pub locations_node: NodeRef,
    pub volumes_node: NodeRef,
    pub sidebar_content_node: NodeRef,
    pub locations_region_node: NodeRef,
    pub sidebar_region: RwSignal<Option<super::keyboard::SidebarRegion>, LocalStorage>,
    pub sidebar_cursor: RwSignal<Option<super::keyboard::SidebarRow>, LocalStorage>,
    pub sidebar_reveal_volumes: RwSignal<bool, LocalStorage>,
    pub tasks: TaskOwner,
    pub owner: Owner,
    pub window: Option<WindowHandle>,
    pub volumes: volumes::Service,
    pub preferences: preferences::Service,
    pub new_window: Rc<dyn Fn(PathBuf)>,
    pub inspections: RwSignal<gds::ui::file_inspection::Cache, LocalStorage>,
    pub terminals: super::terminals::Dock,
    pub connector: crate::features::connect_server::Connector,
    pub menus: super::menus::Menus,
    pub picker_result: Option<Rc<dyn Fn(Vec<PathBuf>)>>,
    pub pending_focus: RefCell<Option<zgui::view::time::FrameHandle>>,
    pub typeahead: RefCell<super::typeahead::TypeAhead>,
}

impl Browser {
    pub(crate) fn new(
        launch: LaunchOptions,
        volumes: volumes::Service,
        new_window: Rc<dyn Fn(PathBuf)>,
    ) -> Self {
        Self::new_with_output(launch, volumes, new_window, None)
    }

    pub(super) fn new_with_picker_result(
        launch: LaunchOptions,
        volumes: volumes::Service,
        new_window: Rc<dyn Fn(PathBuf)>,
        result: Rc<dyn Fn(Vec<PathBuf>)>,
    ) -> Self {
        Self::new_with_output(launch, volumes, new_window, Some(result))
    }

    fn new_with_output(
        launch: LaunchOptions,
        volumes: volumes::Service,
        new_window: Rc<dyn Fn(PathBuf)>,
        picker_result: Option<Rc<dyn Fn(Vec<PathBuf>)>>,
    ) -> Self {
        let LaunchOptions {
            places,
            start,
            home,
            picker,
        } = launch;
        let (places, notice) = match places {
            Ok(places) => (places, None),
            Err(error) => (Places::default(), Some(error.to_string())),
        };
        let location = Location::from(start.as_path());
        let mut tabs = if picker.is_none() {
            places
                .pinned
                .iter()
                .enumerate()
                .map(|(index, pinned)| {
                    Tab::new(index as u64 + 1, pinned.clone(), &places, true).restored()
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let active = tabs
            .iter()
            .position(|tab| tab.navigation.location() == &location)
            .unwrap_or_else(|| {
                tabs.push(Tab::new(
                    tabs.len() as u64 + 1,
                    location.clone(),
                    &places,
                    false,
                ));
                tabs.len() - 1
            });
        let next_tab = tabs.len() as u64 + 1;
        let has_portal_title = picker
            .as_ref()
            .and_then(|options| options.portal.as_ref())
            .is_some_and(|request| !request.title.is_empty());
        let save_name = picker
            .as_ref()
            .and_then(|options| options.portal.as_ref())
            .and_then(|request| match &request.mode {
                gds::service::desktop::protocol::chooser::Mode::Save { name } => {
                    Some(name.to_string_lossy().into_owned())
                }
                _ => None,
            })
            .unwrap_or_default();
        let owner = Owner::current().expect("Files workspace is created inside its window owner");
        let tasks = Gds::current().tasks().scope().named_child("Files window");
        let connector = crate::features::connect_server::Connector::new((*tasks).clone());
        let preference_service = preferences::Service::current();
        let inspections = RwSignal::new_local(gds::ui::file_inspection::Cache::new(
            preference_service.snapshot().files.preview.cache_options(),
        ));
        let terminals = super::terminals::Dock::new();
        let sidebar_tabs_height = preference_service
            .snapshot()
            .files
            .sidebar
            .tabs_height
            .filter(|height| height.is_finite() && *height > 0.0);
        let notice = RwSignal::new_local(notice);
        let apply = preference_service.clone();
        let settings =
            crate::widgets::settings::View::new(preference_service.state(), move |preferences| {
                if let Err(error) = apply.update(preferences) {
                    notice.set(Some(error.to_string()));
                }
            });
        let window = try_use_window();
        let (initial_width, initial_content_width) = resolve_initial_widths(
            window
                .as_ref()
                .map(|win| win.size().get_untracked().width.0),
        );
        let initial_height = resolve_initial_height(
            window
                .as_ref()
                .map(|win| win.size().get_untracked().height.0),
        );
        let window_width = RwSignal::new_local(initial_width);
        let window_height = RwSignal::new_local(initial_height);
        let sidebar = RwSignal::new_local(true);
        let (sidebar_presentation, sidebar_width) =
            owner.with(|| sample_workspace_sidebar(sidebar, window_width));
        let initial_widths = Self::column_widths_map(&places, &location);
        let browser = Self(Rc::new(Workspace {
            alive: Cell::new(true),
            details: RwSignal::new_local(places.details_visible(location.clone())),
            state: RefCell::new(State {
                tabs,
                active,
                next_tab,
                home,
                places,
                history: Default::default(),
                picker: picker.map(pick_files::Session::new),
                operation: Operation::Idle,
                saving_places: false,
                favorites_reload: None,
                pending_device_removals: Vec::new(),
            }),
            revision: RwSignal::new_local(0),
            selection: RwSignal::new_local(0),
            column_widths: RwSignal::new_local(initial_widths),
            pages: RwSignal::new_local(Pages::default()),
            window_width,
            window_height,
            content_width: RwSignal::new_local(initial_content_width),
            footer_filter_open: RwSignal::new_local(false),
            filter_focus_request: RwSignal::new_local(None),
            micro_locations_expanded: RwSignal::new_local(false),
            address_presentation: RwSignal::new_local(AddressPresentation::Breadcrumbs),
            sidebar_tabs_height: RwSignal::new_local(sidebar_tabs_height),
            settings,
            sidebar,
            sidebar_presentation,
            sidebar_width,
            terminal: RwSignal::new_local(false),
            background_tasks: RwSignal::new_local(BackgroundTasks::default()),
            background_tasks_revision: RwSignal::new_local(0),
            drag_sources: DragSources::new(),
            notice,
            prompt: RwSignal::new_local(Prompt::Closed),
            prompt_generation: RwSignal::new_local(0),
            confirm_options: RwSignal::new_local(ConfirmOptions::default()),
            prompt_decisions: PromptDecisionSlot::default(),
            address: RwSignal::new_local(location.display_address()),
            filter: RwSignal::new_local(String::new()),
            save_name: RwSignal::new_local(save_name),
            list: NodeRef::new(),
            content_focus: NodeRef::new(),
            root_focus: NodeRef::new(),
            list_focused: Cell::new(false),
            list_focus_visible: RwSignal::new_local(false),
            settings_node: NodeRef::new(),
            address_node: NodeRef::new(),
            filter_node: NodeRef::new(),
            tabs_node: NodeRef::new(),
            locations_node: NodeRef::new(),
            volumes_node: NodeRef::new(),
            sidebar_content_node: NodeRef::new(),
            locations_region_node: NodeRef::new(),
            sidebar_region: RwSignal::new_local(None),
            sidebar_cursor: RwSignal::new_local(None),
            sidebar_reveal_volumes: RwSignal::new_local(false),
            tasks,
            connector,
            inspections,
            terminals,
            menus: super::menus::Menus::new(),
            picker_result,
            pending_focus: RefCell::new(None),
            typeahead: RefCell::new(Default::default()),
            owner,
            window: window.clone(),
            volumes,
            preferences: preference_service,
            new_window,
        }));
        browser
            .0
            .state
            .borrow()
            .places
            .drain_on_close(&Gds::current().tasks().scope(), browser.0.tasks.token());
        let weak = browser.downgrade();
        on_cleanup_local(move || {
            if let Some(browser) = weak.0.upgrade() {
                browser.alive.set(false);
                *browser.pending_focus.borrow_mut() = None;
                browser.drag_sources.clear();
                browser
                    .background_tasks
                    .with_untracked(BackgroundTasks::cancel_all);
                browser.state.borrow().operation.cancel();
                browser.tasks.cancel();
            }
        });
        if window.is_some() {
            let close = browser.downgrade();
            let guard = on_close_request(move || {
                if let Some(browser) = close.upgrade() {
                    browser.cancel_picker();
                }
                CloseResponse::Close
            });
            on_cleanup_local(move || drop(guard));
        }
        if let Some(win) = window {
            let focused = win.focused();
            let focus_effect = RenderEffect::new(move |_| {
                if focused.get() {
                    Clipboard::current().refresh();
                }
            });
            on_cleanup_local(move || drop(focus_effect));

            if !has_portal_title {
                let ready_win = win.clone();
                let title_win = win;
                let prefs = browser.0.preferences.state();
                let presented = ready_win.presented();
                observe_window_title(
                    prefs,
                    move || {
                        let _ = presented.get();
                        ready_win.is_open()
                    },
                    move |title| title_win.set_title(title),
                );
            }
        }
        browser.observe_services();
        browser.observe_terminals();
        browser.initialize_picker();
        let ids =
            browser.read_untracked(|state| state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>());
        for id in ids {
            browser.refresh_tab(id);
        }
        browser.focus_content_after_mount();
        browser
    }

    pub(super) fn focus_content_after_mount(&self) {
        if self.pane_untracked() == Pane::Files
            && matches!(
                self.0.address_presentation.get_untracked(),
                AddressPresentation::Breadcrumbs
            )
            && self.0.content_focus.is_bound()
        {
            self.focus_content();
            return;
        }
        let weak = self.downgrade();
        let handle = self.0.owner.with(|| {
            request_frame(move |_| {
                if let Some(browser) = weak.upgrade()
                    && browser.pane_untracked() == Pane::Files
                    && matches!(
                        browser.0.address_presentation.get_untracked(),
                        AddressPresentation::Breadcrumbs
                    )
                {
                    browser.focus_content();
                }
            })
        });
        *self.0.pending_focus.borrow_mut() = Some(handle);
    }

    pub(super) fn focus_content(&self) {
        if self.0.content_focus.is_bound() {
            self.0.content_focus.focus();
        } else {
            self.0.root_focus.focus();
        }
    }

    pub(super) fn reset_typeahead(&self) {
        self.0.typeahead.borrow_mut().reset();
    }

    pub(super) fn pane(&self) -> Pane {
        self.0.pages.with(Pages::current)
    }

    pub(super) fn pane_untracked(&self) -> Pane {
        self.0.pages.with_untracked(Pages::current)
    }

    pub(super) fn show_files(&self) {
        self.0.pages.update(|pages| pages.open(Pane::Files));
        self.focus_content_after_mount();
    }

    pub(super) fn back_page(&self) {
        self.0.pages.update(Pages::back);
        self.focus_page_after_mount();
    }

    pub(super) fn open_page(&self, pane: Pane) {
        self.finish_address_edit();
        self.0.pages.update(|pages| pages.open(pane));
        self.focus_page_after_mount();
    }

    fn focus_page_after_mount(&self) {
        if self.pane_untracked() == Pane::Files {
            self.focus_content_after_mount();
        } else {
            let node = self.0.settings_node;
            let handle = self.0.owner.with(|| request_frame(move |_| node.focus()));
            *self.0.pending_focus.borrow_mut() = Some(handle);
        }
    }

    pub(super) fn sidebar_contains_focus(&self) -> bool {
        if self.focused_sidebar_region().is_some() {
            return true;
        }
        let Some(focused) = zgui::view::current_host()
            .and_then(|host| host.focused().try_get_untracked().flatten())
        else {
            return false;
        };
        [
            self.0.sidebar_content_node,
            self.0.locations_region_node,
            self.0.tabs_node,
            self.0.volumes_node,
            self.0.locations_node,
        ]
        .into_iter()
        .any(|node| node.get_untracked() == Some(focused) || node.contains(focused))
    }

    pub(super) fn toggle_sidebar(&self) {
        if self.width_tier_untracked() == gds::ui::window::WidthTier::Micro {
            return;
        }
        let next = !self.0.sidebar.get_untracked();
        let restore_focus = !next && self.sidebar_contains_focus();
        self.0.sidebar.set(next);
        if restore_focus {
            self.0.sidebar_region.set(None);
            if self.pane_untracked() == Pane::Files {
                self.focus_content();
            } else if self.0.settings_node.is_bound() {
                self.0.settings_node.focus();
            } else {
                self.focus_content();
            }
        }
    }

    pub(super) fn toggle_settings(&self) {
        if self.pane_untracked() == Pane::Settings {
            self.back_page();
        } else {
            self.show_settings();
        }
    }

    pub(super) fn show_settings(&self) {
        self.open_page(Pane::Settings);
    }

    pub(crate) fn downgrade(&self) -> WeakBrowser {
        WeakBrowser(Rc::downgrade(&self.0))
    }

    pub(super) fn track(&self) {
        let _ = self.0.revision.get();
        let _ = self.0.selection.get();
    }

    pub(super) fn changed(&self) {
        self.0
            .revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    pub(super) fn read<R>(&self, read: impl FnOnce(&State) -> R) -> R {
        self.track();
        read(&self.0.state.borrow())
    }

    pub(super) fn read_structure<R>(&self, read: impl FnOnce(&State) -> R) -> R {
        let _ = self.0.revision.get();
        read(&self.0.state.borrow())
    }

    pub(super) fn read_untracked<R>(&self, read: impl FnOnce(&State) -> R) -> R {
        read(&self.0.state.borrow())
    }

    pub(super) fn update_selection<R>(&self, edit: impl FnOnce(&mut State) -> R) -> R {
        let result = edit(&mut self.0.state.borrow_mut());
        self.0
            .selection
            .update(|revision| *revision = revision.wrapping_add(1));
        result
    }

    pub(super) fn update<R>(&self, edit: impl FnOnce(&mut State) -> R) -> R {
        let (value, op_sync) = {
            let mut state = self.0.state.borrow_mut();
            let prior = match &state.operation {
                Operation::Idle => None,
                Operation::Running { label, cancel } => {
                    Some((label.clone(), cancel.is_cancelled()))
                }
            };
            let value = edit(&mut state);
            let next = match &state.operation {
                Operation::Idle => None,
                Operation::Running { label, cancel } => Some((label.clone(), cancel.clone())),
            };
            let next_sig = next
                .as_ref()
                .map(|(label, cancel)| (label.as_str(), cancel.is_cancelled()));
            let prior_sig = prior
                .as_ref()
                .map(|(label, cancelled)| (label.as_str(), *cancelled));
            (value, (prior_sig != next_sig).then_some(next))
        };
        if let Some(op_state) = op_sync {
            self.0.background_tasks.update(|tasks| {
                tasks.sync(FILES_TASK_KEY, op_state);
            });
            self.bump_background_tasks_revision();
        }
        self.changed();
        value
    }

    pub(crate) fn report_notice(&self, notice: impl Into<String>) {
        self.0.notice.set(Some(notice.into()));
    }

    pub(super) fn report_success(&self, message: impl Into<String>) {
        gds::ui::feedback::notifications::success(message);
    }

    pub(super) fn scope(&self) -> Scope {
        (*self.0.tasks).clone()
    }

    pub(super) fn work<T: Send + 'static>(
        &self,
        name: &'static str,
        work: impl Future<Output = Result<T, String>> + Send + 'static,
        finished: impl FnOnce(&Browser, Result<T, String>) + 'static,
    ) {
        let browser = self.downgrade();
        let task = self.0.tasks.spawn_named(name, work);
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(std::convert::identity);
                if let Some(browser) = browser.upgrade() {
                    finished(&browser, result);
                }
            }));
        });
    }

    pub(super) fn blocking<T: Send + 'static>(
        &self,
        name: &'static str,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
        finished: impl FnOnce(&Browser, Result<T, String>) + 'static,
    ) {
        let browser = self.downgrade();
        let task = self.0.tasks.spawn_blocking_named(name, work);
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(std::convert::identity);
                if let Some(browser) = browser.upgrade() {
                    finished(&browser, result);
                }
            }));
        });
    }

    pub(super) fn save_places(&self) {
        let save = {
            let mut state = self.0.state.borrow_mut();
            if state.saving_places {
                return;
            }
            let Some(save) = state.places.take_save() else {
                return;
            };
            state.saving_places = true;
            save
        };
        self.work(
            "Save Files places",
            async move { save.run().await.map_err(|error| error.to_string()) },
            |browser, result| {
                browser.update(|state| {
                    state.saving_places = false;
                    if let Ok(snapshot) = result.as_ref() {
                        state.places.accept(snapshot.clone());
                    }
                });
                match result {
                    Ok(_) => browser.save_places(),
                    Err(error) => browser.report_notice(error),
                }
            },
        );
    }

    pub(super) fn edit_preferences(&self, edit: impl FnOnce(&mut preferences::Preferences)) {
        if let Err(error) = self.0.preferences.edit(edit) {
            self.report_notice(error.to_string());
        }
    }

    pub(super) fn open_prompt(&self, prompt: Prompt) {
        self.open_prompt_with_options(prompt, ConfirmOptions::default());
    }

    fn open_prompt_with_options(&self, prompt: Prompt, options: ConfirmOptions) {
        debug_assert!(!matches!(prompt, Prompt::Closed));
        self.0.menus.close();
        self.0.confirm_options.set(options);
        self.0.prompt.set(prompt);
        self.0
            .prompt_generation
            .update(|generation| *generation = generation.wrapping_add(1));
    }

    pub(super) fn confirm_with_options(
        &self,
        title: String,
        message: String,
        primary: String,
        options: ConfirmOptions,
        accept: impl Fn() + 'static,
    ) {
        self.open_prompt_with_options(
            Prompt::Confirm {
                title,
                message,
                primary,
                accept: Rc::new(accept),
            },
            options,
        );
    }

    pub(super) fn prompt_name(
        &self,
        title: String,
        initial: String,
        submit: impl Fn(String) + 'static,
    ) {
        self.name_form(
            NameForm::new(NamePurpose::Generic, title, initial.into()),
            move |name| {
                submit(
                    name.into_string()
                        .expect("generic name comes from a UTF-8 text field"),
                );
            },
        );
    }

    pub(super) fn name_form(&self, form: NameForm, submit: impl Fn(OsString) + 'static) {
        self.open_prompt(Prompt::Name {
            form,
            submit: Rc::new(submit),
            cancel: None,
        });
    }

    pub(super) fn name_form_with_cancel(
        &self,
        form: NameForm,
        submit: impl Fn(OsString) + 'static,
        cancel: impl Fn() + 'static,
    ) {
        self.open_prompt(Prompt::Name {
            form,
            submit: Rc::new(submit),
            cancel: Some(Rc::new(cancel)),
        });
    }

    pub(super) fn cancel_prompt(&self) {
        let prompt = self.0.prompt.get_untracked();
        if matches!(prompt, Prompt::Closed) {
            return;
        }
        self.0.prompt_decisions.clear();
        self.0.prompt.set(Prompt::Closed);
        if let Prompt::Name {
            cancel: Some(cancel),
            ..
        } = prompt
        {
            cancel();
        }
    }
}

impl Browser {
    pub(super) fn column_widths_map(places: &Places, location: &Location) -> HashMap<Column, f32> {
        places
            .column_widths(location.clone())
            .into_iter()
            .filter_map(|(id, width)| Column::from_id(&id).map(|column| (column, width)))
            .collect()
    }

    fn observe_services(&self) {
        let previous = RefCell::new(self.0.preferences.snapshot());
        let browser = self.clone();
        let pref_fx = RenderEffect::new(move |_| {
            let preferences = browser.0.preferences.state().get();
            let prior = previous.replace(preferences.clone());
            browser.apply_terminal_preferences(&preferences);
            if prior.files.preview != preferences.files.preview {
                browser
                    .0
                    .inspections
                    .set(gds::ui::file_inspection::Cache::new(
                        preferences.files.preview.cache_options(),
                    ));
            }
            if prior.files.show_hidden != preferences.files.show_hidden
                || prior.files.watching != preferences.files.watching
                || prior.files.streaming != preferences.files.streaming
            {
                let ids = browser
                    .0
                    .state
                    .borrow()
                    .tabs
                    .iter()
                    .filter(|tab| {
                        matches!(tab.contents, Contents::Directory | Contents::Remote { .. })
                    })
                    .map(|tab| tab.id)
                    .collect::<Vec<_>>();
                for id in ids {
                    browser.refresh_tab(id);
                }
            } else if prior.files.enrichment != preferences.files.enrichment
                || prior.files.probe_budget != preferences.files.probe_budget
            {
                let reset_running = prior.files.enrichment.content_probing
                    != preferences.files.enrichment.content_probing;
                let ids = {
                    let mut state = browser.0.state.borrow_mut();
                    state
                        .tabs
                        .iter_mut()
                        .map(|tab| {
                            if reset_running {
                                tab.enrichment = None;
                            }
                            tab.id
                        })
                        .collect::<Vec<_>>()
                };
                for id in ids {
                    browser.enrich_directory(id);
                }
            }
            if prior.tags != preferences.tags {
                let ids = browser
                    .0
                    .state
                    .borrow()
                    .tabs
                    .iter()
                    .filter(|tab| matches!(tab.navigation.location(), Location::Tags(_)))
                    .map(|tab| tab.id)
                    .collect::<Vec<_>>();
                for id in ids {
                    browser.refresh_tab(id);
                }
            }
        });
        on_cleanup_local(move || drop(pref_fx));

        let browser = self.clone();
        let volume_fx = RenderEffect::new(move |_| {
            let _ = browser.0.volumes.snapshot();
            let (collections, restored) = {
                let state = browser.0.state.borrow();
                let collections = state
                    .tabs
                    .iter()
                    .filter(|tab| matches!(tab.contents, Contents::Volumes))
                    .map(|tab| tab.id)
                    .collect::<Vec<_>>();
                let tab = state.tab();
                let navigation = &tab.navigation;
                let location = navigation.location();
                let restored = matches!(location, Location::Volume { .. })
                    && navigation.path().is_none()
                    && navigation.pending_path().is_none()
                    && navigation
                        .pending_location()
                        .is_none_or(|pending| pending == location)
                    && browser.resolve_path(location).is_some();
                (collections, restored.then_some(tab.id))
            };
            for id in collections {
                browser.reproject(id);
            }
            if let Some(id) = restored {
                browser.refresh_tab(id);
            }
        });
        on_cleanup_local(move || drop(volume_fx));

        let browser = self.clone();
        let error_fx = RenderEffect::new(move |_| {
            if let Some(error) = browser.0.preferences.error().get() {
                browser.report_notice(error);
            }
        });
        on_cleanup_local(move || drop(error_fx));

        let browser = self.clone();
        let lang_fx = RenderEffect::new(move |_| {
            let _ = gds::app::Gds::current().i18n().language();
            let ids = browser
                .0
                .state
                .borrow()
                .tabs
                .iter()
                .filter(|tab| tab.sort.key == crate::entities::file::sorting::Key::DetailedType)
                .map(|tab| tab.id)
                .collect::<Vec<_>>();
            for id in ids {
                browser.reproject(id);
            }
        });
        on_cleanup_local(move || drop(lang_fx));
    }
}

/// Shared workspace accessors for synchronized sidebar presentation, concurrent background
/// tasks, active drag-source ghosting, and preference helpers.
impl Browser {
    pub(super) fn sidebar_presentation(&self) -> Signal<SidebarPresentation, LocalStorage> {
        self.0.sidebar_presentation
    }

    pub(super) fn sidebar_width(&self) -> Signal<f32, LocalStorage> {
        self.0.sidebar_width
    }

    pub(super) fn tags_enabled(&self) -> bool {
        self.0.preferences.state().with(|prefs| prefs.tags.enabled)
    }

    pub(super) fn tags_enabled_untracked(&self) -> bool {
        self.0.preferences.snapshot().tags.enabled
    }

    fn bump_background_tasks_revision(&self) {
        self.0
            .background_tasks_revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    fn mirror_files_operation(&self, task: Option<(String, CancellationToken)>) {
        let mut state = self.0.state.borrow_mut();
        state.operation = match task {
            Some((label, cancel)) => Operation::Running { label, cancel },
            None => Operation::Idle,
        };
    }

    pub(super) fn background_tasks_revision(&self) -> ReadSignal<u64, LocalStorage> {
        self.0.background_tasks_revision.read_only()
    }

    pub(super) fn background_task_list(&self) -> Vec<BackgroundTask> {
        let _ = self.0.background_tasks_revision.get();
        self.0.background_tasks.with(|tasks| tasks.items().to_vec())
    }

    pub(super) fn has_background_tasks_untracked(&self) -> bool {
        self.0
            .background_tasks
            .with_untracked(|tasks| !tasks.is_empty())
    }

    pub(super) fn is_background_task_busy(&self, key: &str) -> bool {
        let _ = self.0.background_tasks_revision.get();
        self.0.background_tasks.with(|tasks| tasks.is_busy(key))
    }

    pub(super) fn is_background_task_busy_untracked(&self, key: &str) -> bool {
        self.0
            .background_tasks
            .with_untracked(|tasks| tasks.is_busy(key))
    }

    pub(super) fn begin_background_task(
        &self,
        key: impl Into<String>,
        label: impl Into<String>,
    ) -> Option<CancellationToken> {
        let key = key.into();
        let label = label.into();
        let cancel = self.0.tasks.token().child_token();
        if self.is_background_task_busy_untracked(&key) {
            return None;
        }
        if key == FILES_TASK_KEY {
            if self.read_untracked(|state| state.operation.busy()) {
                return None;
            }
            self.mirror_files_operation(Some((label.clone(), cancel.clone())));
        }
        let mut started = false;
        let label_for_task = label;
        let cancel_for_task = cancel.clone();
        let key_for_task = key;
        self.0.background_tasks.update(|tasks| {
            started = tasks.begin(key_for_task, label_for_task, cancel_for_task);
        });
        if !started {
            return None;
        }
        self.bump_background_tasks_revision();
        self.changed();
        Some(cancel)
    }

    pub(super) fn set_background_task_detail(&self, key: &str, detail: Option<String>) {
        let mut updated = false;
        self.0.background_tasks.update(|tasks| {
            updated = tasks.set_detail(key, detail);
        });
        if updated {
            self.bump_background_tasks_revision();
        }
    }

    pub(super) fn cancel_background_task(&self, key: &str) -> bool {
        let cancelled = self
            .0
            .background_tasks
            .with_untracked(|tasks| tasks.cancel(key));
        if key == FILES_TASK_KEY {
            self.read_untracked(|state| state.operation.cancel());
        }
        if cancelled {
            self.bump_background_tasks_revision();
            self.changed();
        }
        cancelled
    }

    pub(super) fn finish_background_task(&self, key: &str) -> Option<BackgroundTask> {
        if key == FILES_TASK_KEY {
            self.mirror_files_operation(None);
        }
        let mut removed = None;
        self.0.background_tasks.update(|tasks| {
            removed = tasks.finish(key);
        });
        if removed.is_some() {
            self.bump_background_tasks_revision();
            self.changed();
        }
        removed
    }

    pub(super) fn drag_sources(&self) -> ReadSignal<HashSet<PathBuf>, LocalStorage> {
        self.0.drag_sources.read_only()
    }

    pub(super) fn set_drag_sources(&self, paths: impl IntoIterator<Item = PathBuf>) {
        self.0.drag_sources.set(paths);
    }

    pub(super) fn clear_drag_sources(&self) {
        self.0.drag_sources.clear();
    }

    pub(super) fn is_drag_source(&self, path: &Path) -> bool {
        self.0.drag_sources.contains(path)
    }

    pub(super) fn has_drag_sources_untracked(&self) -> bool {
        !self.0.drag_sources.is_empty_untracked()
    }
}

#[cfg(test)]
mod tests;
