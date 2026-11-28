//! Outermost workspace `Browser` facade, lifecycle wiring, service observers, and desktop commands.

use std::{
    cell::{Cell, RefCell},
    ops::Deref,
    path::PathBuf,
    rc::Rc,
};

#[cfg(test)]
use gds::ui::{drag::ExternalFileDrop, window::WidthTier};
use gds::{
    app::Gds,
    service::desktop::file_manager::{Command, Request},
};
use zgui::prelude::*;
use zgui::reactive::RenderEffect;
use zgui::view;

use super::super::{
    navigation::{EnrichmentRunner, Navigator},
    operations::PickerOps,
    panes::{self, TerminalDock},
    projection::Rows,
    state::{
        context::{
            Context, DragSources, LaunchOptions, WeakContext, Workspace, observe_window_title,
            resolve_initial_height, resolve_initial_widths, sample_workspace_sidebar,
        },
        dock::Dock,
        menus::Menus,
        model::{
            AddressPresentation, BackgroundTasks, ConfirmOptions, Contents, Operation, Pages,
            Prompt, State, Tab,
        },
        name_form::PromptDecisionSlot,
        ui_flags::{AccordionSelection, OpenPickerWindow},
    },
};
#[cfg(test)]
use super::super::{operations::PasteOps, projection::Geometry};
use super::view::WorkspaceView;
#[cfg(test)]
use crate::features::file_operations::paste;
use crate::{
    entities::{
        location::{Location, places::Places, volumes},
        preferences,
    },
    features::pick_files,
    shared::lib::clipboard::Clipboard,
};

#[derive(Clone)]
pub(crate) struct Browser {
    ctx: Context,
}

impl Deref for Browser {
    type Target = Context;

    fn deref(&self) -> &Context {
        &self.ctx
    }
}

#[derive(Clone)]
pub(crate) struct WeakBrowser(WeakContext);

impl WeakBrowser {
    pub(crate) fn upgrade(&self) -> Option<Browser> {
        self.0.upgrade().map(Browser::from_context)
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn from_context(ctx: Context) -> Self {
        Self { ctx }
    }

    pub(crate) fn downgrade(&self) -> WeakBrowser {
        WeakBrowser(self.ctx.downgrade())
    }

    pub(crate) fn view(&self) -> AnyView {
        WorkspaceView(&self.ctx).view()
    }

    pub(crate) fn new(
        launch: LaunchOptions,
        volumes: volumes::Service,
        new_window: Rc<dyn Fn(PathBuf)>,
    ) -> Self {
        Self::new_with_output(launch, volumes, new_window, None)
    }

    fn new_with_picker_result(
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
        let inspections = RwSignal::new_local(gds::ui::file_inspection::Cache::with_scope(
            (*tasks).clone(),
            preference_service.snapshot().files.preview.cache_options(),
        ));
        let terminals = Dock::new();
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
        let initial_widths = Context::column_widths_map(&places, &location);
        let picker_volumes = volumes.clone();
        let picker_new_window = Rc::clone(&new_window);
        let open_picker_window: OpenPickerWindow = Rc::new(move |title, launch, callback| {
            let Some(windows) = try_use_windows() else {
                return;
            };
            let launch_slot = RefCell::new(Some(launch));
            let volumes = picker_volumes.clone();
            let new_window = Rc::clone(&picker_new_window);
            let window_opts = gds::ui::window::workspace_options(title, None);
            windows.open(window_opts, move || {
                let _ = Gds::current().appearance().install_window_sheet();
                let Some(launch) = launch_slot.borrow_mut().take() else {
                    return AnyView::new(view! { box() });
                };
                let browser = Browser::new_with_picker_result(
                    launch,
                    volumes.clone(),
                    Rc::clone(&new_window),
                    Rc::clone(&callback),
                );
                WorkspaceView(&browser.ctx).view()
            });
        });
        let browser = Self::from_context(Context(Rc::new(Workspace {
            alive: Cell::new(true),
            details: RwSignal::new_local(places.details_visible(location.clone())),
            details_revision: RwSignal::new_local(0),
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
            sidebar_in_accordion: RwSignal::new_local(false),
            sidebar_accordion: RwSignal::new_local(AccordionSelection::Auto),
            address_presentation: RwSignal::new_local(AddressPresentation::Breadcrumbs),
            settings,
            sidebar,
            sidebar_presentation,
            sidebar_width,
            terminal: RwSignal::new_local(false),
            background_tasks: RwSignal::new_local(BackgroundTasks::default()),
            background_tasks_revision: RwSignal::new_local(0),
            drag_sources: DragSources::new(),
            native_drag_active: Cell::new(false),
            notice,
            prompt: RwSignal::new_local(Prompt::Closed),
            prompt_generation: RwSignal::new_local(0),
            confirm_options: RwSignal::new_local(ConfirmOptions::default()),
            prompt_decisions: PromptDecisionSlot::default(),
            address: RwSignal::new_local(location.display_address()),
            filter: RwSignal::new_local(String::new()),
            save_name: RwSignal::new_local(save_name),
            save_input_node: NodeRef::new(),
            list: NodeRef::new(),
            content_focus: NodeRef::new(),
            root_focus: NodeRef::new(),
            list_focused: Cell::new(false),
            list_focus_visible: RwSignal::new_local(false),
            settings_node: NodeRef::new(),
            address_node: NodeRef::new(),
            filter_node: NodeRef::new(),
            tabs_node: NodeRef::new(),
            quick_access_node: NodeRef::new(),
            locations_node: NodeRef::new(),
            volumes_node: NodeRef::new(),
            sidebar_content_node: NodeRef::new(),
            locations_region_node: NodeRef::new(),
            sidebar_region: RwSignal::new_local(None),
            sidebar_cursor: RwSignal::new_local(None),
            sidebar_reveal_quick_access: RwSignal::new_local(false),
            sidebar_reveal_volumes: RwSignal::new_local(false),
            tasks,
            connector,
            inspections,
            external_thumbnailers: RefCell::new(None),
            external_thumbnailer_probe: RefCell::new(None),
            external_thumbnailer_gen: Cell::new(0),
            terminals,
            menus: Menus::new(),
            picker_result,
            pending_focus: RefCell::new(None),
            typeahead: RefCell::new(Default::default()),
            owner,
            window: window.clone(),
            volumes,
            preferences: preference_service,
            new_window,
            open_picker_window,
        })));
        browser
            .0
            .state
            .borrow()
            .places
            .drain_on_close(&Gds::current().tasks().scope(), browser.0.tasks.token());
        let weak = browser.downgrade();
        on_cleanup_local(move || {
            if let Some(browser) = weak.upgrade() {
                browser.0.alive.set(false);
                *browser.0.pending_focus.borrow_mut() = None;
                *browser.0.external_thumbnailer_probe.borrow_mut() = None;
                browser.0.drag_sources.clear();
                browser
                    .0
                    .background_tasks
                    .with_untracked(BackgroundTasks::cancel_all);
                browser.0.state.borrow().operation.cancel();
                browser.0.tasks.cancel();
            }
        });
        if window.is_some() {
            let close = browser.downgrade();
            let guard = on_close_request(move || {
                if let Some(browser) = close.upgrade() {
                    PickerOps(&browser.ctx).cancel_picker();
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
        TerminalDock(&browser.ctx).observe_terminals();
        PickerOps(&browser.ctx).initialize_picker();
        let ids =
            browser.read_untracked(|state| state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>());
        let navigator = Navigator(&browser.ctx);
        for id in ids {
            navigator.refresh_tab(id);
        }
        browser.focus_content_after_mount();
        browser
    }

    fn sync_external_thumbnailers(&self, preferences: &preferences::Preferences) {
        let probe_gen = self.0.external_thumbnailer_gen.get().wrapping_add(1);
        self.0.external_thumbnailer_gen.set(probe_gen);
        drop(self.0.external_thumbnailer_probe.borrow_mut().take());

        if !preferences.thumbnailers.enabled {
            let had_runner = self.0.external_thumbnailers.borrow_mut().take().is_some();
            if had_runner {
                let next_cache = self.0.owner.with(|| {
                    gds::ui::file_inspection::Cache::with_scope(
                        (*self.0.tasks).clone(),
                        preferences.files.preview.cache_options_with(None),
                    )
                });
                self.0.inspections.set(next_cache);
                self.bump_details_revision();
                self.changed();
            }
            return;
        }

        let probe_owner = self.0.tasks.named_child("Load external thumbnailers");
        let cancel = probe_owner.token();
        let use_bwrap = preferences.thumbnailers.use_bwrap;
        let bwrap = preferences.thumbnailers.commands.bwrap.clone();
        let task = probe_owner.spawn_cancellable_named("Load external thumbnailers", async move {
            gds::service::file_inspection::ExternalThumbnailers::load(
                use_bwrap,
                bwrap.as_deref(),
                &cancel,
            )
            .await
        });
        *self.0.external_thumbnailer_probe.borrow_mut() = Some(probe_owner);
        let weak = self.downgrade();
        self.0.owner.with(move || {
            drop(zgui::reactive::spawn_local(async move {
                let result = task.await;
                let Some(browser) = weak.upgrade() else {
                    return;
                };
                if browser.0.external_thumbnailer_gen.get() != probe_gen {
                    return;
                }
                drop(browser.0.external_thumbnailer_probe.borrow_mut().take());
                let prefs = browser.0.preferences.snapshot();
                if !prefs.thumbnailers.enabled {
                    return;
                }
                let next = result.ok().and_then(Result::ok).map(std::sync::Arc::new);
                let changed = *browser.0.external_thumbnailers.borrow() != next;
                *browser.0.external_thumbnailers.borrow_mut() = next.clone();
                if changed {
                    let next_cache = browser.0.owner.with(|| {
                        gds::ui::file_inspection::Cache::with_scope(
                            (*browser.0.tasks).clone(),
                            prefs.files.preview.cache_options_with(next),
                        )
                    });
                    browser.0.inspections.set(next_cache);
                    browser.bump_details_revision();
                    browser.changed();
                }
            }));
        });
    }

    fn observe_services(&self) {
        let initial_prefs = self.0.preferences.snapshot();
        if initial_prefs.thumbnailers.enabled {
            self.sync_external_thumbnailers(&initial_prefs);
        }
        let previous = RefCell::new(initial_prefs);
        let browser = self.clone();
        let pref_fx = RenderEffect::new(move |_| {
            let preferences = browser.0.preferences.state().get();
            let prior = previous.replace(preferences.clone());
            TerminalDock(&browser.ctx).apply_terminal_preferences(&preferences);
            if prior.thumbnailers != preferences.thumbnailers {
                browser.sync_external_thumbnailers(&preferences);
            } else if prior.files.preview != preferences.files.preview {
                let external = browser.0.external_thumbnailers.borrow().clone();
                let next_cache = browser.0.owner.with(|| {
                    gds::ui::file_inspection::Cache::with_scope(
                        (*browser.0.tasks).clone(),
                        preferences.files.preview.cache_options_with(external),
                    )
                });
                browser.0.inspections.set(next_cache);
                browser.bump_details_revision();
                browser.changed();
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
                let navigator = Navigator(&browser.ctx);
                for id in ids {
                    navigator.refresh_tab(id);
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
                let enrichment = EnrichmentRunner(&browser.ctx);
                for id in ids {
                    enrichment.enrich_directory(id);
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
                let navigator = Navigator(&browser.ctx);
                for id in ids {
                    navigator.refresh_tab(id);
                }
            }
        });
        on_cleanup_local(move || drop(pref_fx));

        let browser = self.clone();
        let volume_fx = RenderEffect::new(move |_| {
            let _ = browser.0.volumes.snapshot();
            let rows = Rows(&browser.ctx);
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
                    && rows.resolve_path(location).is_some();
                (collections, restored.then_some(tab.id))
            };
            for id in collections {
                rows.reproject(id);
            }
            if let Some(id) = restored {
                Navigator(&browser.ctx).refresh_tab(id);
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
            let rows = Rows(&browser.ctx);
            for id in ids {
                rows.reproject(id);
            }
        });
        on_cleanup_local(move || drop(lang_fx));
        self.ctx.bind_places_sync();
    }

    pub(crate) fn open_properties(
        paths: Vec<PathBuf>,
        scope: gds::service::task_manager::Scope,
        startup_id: &str,
    ) {
        let mut token = (!startup_id.is_empty()).then_some(startup_id);
        let mut unique = Vec::with_capacity(paths.len());
        for path in paths {
            if !unique.contains(&path) {
                unique.push(path);
            }
        }
        for path in unique {
            panes::inspection::Properties::open_with_activation(path, scope.clone(), token.take());
        }
    }

    pub(crate) fn apply_desktop_command(&self, command: Command) {
        if self.read_untracked(|state| state.picker.is_some()) {
            return;
        }
        match command.request {
            Request::Activate { .. } => {
                self.show_files();
                self.focus_content();
                if let Some(window) = &self.0.window {
                    if !command.startup_id.is_empty() {
                        gds::ui::window::activate_with_token(window, &command.startup_id);
                    } else {
                        window.focus();
                    }
                }
            }
            Request::ShowProperties { paths } => {
                Self::open_properties(paths, self.scope(), &command.startup_id);
            }
            Request::OpenLocations { uris, new_window } => {
                self.open_locations(uris, new_window, &command.startup_id);
            }
            Request::ShowFolders { uris } => {
                self.open_locations(uris, false, &command.startup_id);
            }
            Request::NewWindow { directory } => {
                self.apply_local_targets(Target::folders(vec![directory]), &command.startup_id);
            }
            Request::ShowItems { paths } => {
                self.apply_local_targets(Target::items(paths), &command.startup_id);
            }
        }
    }

    fn open_locations(&self, uris: Vec<String>, replace_first: bool, startup_id: &str) {
        let navigator = Navigator(&self.ctx);
        self.show_files();
        let mut locations = Vec::with_capacity(uris.len());
        for uri in uris {
            match crate::entities::location::scheme::parse(&uri) {
                Ok(location) => {
                    if !locations.contains(&location) {
                        locations.push(location);
                    }
                }
                Err(error) => self.report_notice(error.to_string()),
            }
        }
        for (index, location) in locations.into_iter().enumerate() {
            if replace_first && index == 0 {
                navigator.open(location);
                continue;
            }
            let existing = self.read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .position(|tab| tab.navigation.location() == &location)
            });
            if let Some(tab_index) = existing {
                navigator.select_tab(tab_index);
            } else {
                navigator.new_tab(Some(location));
            }
        }
        self.focus_content();
        if !startup_id.is_empty()
            && let Some(window) = &self.0.window
        {
            gds::ui::window::activate_with_token(window, startup_id);
        }
    }

    fn apply_local_targets(&self, targets: Vec<Target>, startup_id: &str) {
        let navigator = Navigator(&self.ctx);
        self.show_files();
        let rows = Rows(&self.ctx);
        for target in targets {
            let existing = self.read_untracked(|state| {
                state
                    .tabs
                    .iter()
                    .position(|tab| tab.navigation.path() == Some(target.directory.as_path()))
            });
            if let Some(index) = existing {
                navigator.select_tab(index);
            } else {
                navigator.new_tab(Some(Location::from(target.directory.as_path())));
            }
            let reveal_dotfiles = selection_has_dotfile(&target.selection);
            self.update(|state| {
                let tab = state.tab_mut();
                tab.navigation.view_mut().filter.clear();
                tab.reveal_hidden = reveal_dotfiles;
            });
            self.0.filter.set(String::new());
            navigator.reveal_paths(target.selection);
            rows.sync_address();
            navigator.refresh();
        }
        self.focus_content();
        if !startup_id.is_empty()
            && let Some(window) = &self.0.window
        {
            gds::ui::window::activate_with_token(window, startup_id);
        }
    }
}

#[cfg(test)]
impl Browser {
    pub(in crate::pages::workspace) fn context(&self) -> &Context {
        &self.ctx
    }

    pub(in crate::pages::workspace) fn width_tier_untracked(&self) -> WidthTier {
        Geometry(&self.ctx).width_tier_untracked()
    }

    pub(in crate::pages::workspace) fn handle_external_file_drop(
        &self,
        destination: PathBuf,
        drop: ExternalFileDrop,
    ) {
        PasteOps(&self.ctx).handle_external_file_drop(destination, drop);
    }

    pub(in crate::pages::workspace) fn inspect_paste(
        &self,
        transfer: paste::PasteJob,
        cut: bool,
        history: bool,
        finish: Option<Vec<String>>,
    ) {
        PasteOps(&self.ctx).inspect_paste(transfer, cut, history, finish);
    }
}

fn selection_has_dotfile(paths: &[PathBuf]) -> bool {
    paths.iter().any(|path| {
        path.file_name()
            .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
    })
}

struct Target {
    directory: PathBuf,
    selection: Vec<PathBuf>,
}

impl Target {
    fn folders(directories: Vec<PathBuf>) -> Vec<Self> {
        let mut targets = Vec::<Self>::new();
        for directory in directories {
            if !targets.iter().any(|target| target.directory == directory) {
                targets.push(Self {
                    directory,
                    selection: Vec::new(),
                });
            }
        }
        targets
    }

    fn items(paths: Vec<PathBuf>) -> Vec<Self> {
        let mut targets = Vec::<Self>::new();
        for path in paths {
            let directory = path.parent().unwrap_or(&path).to_owned();
            if let Some(target) = targets
                .iter_mut()
                .find(|target| target.directory == directory)
            {
                if !target.selection.contains(&path) {
                    target.selection.push(path);
                }
            } else {
                targets.push(Self {
                    directory,
                    selection: vec![path],
                });
            }
        }
        targets
    }
}

#[cfg(test)]
mod tests;
