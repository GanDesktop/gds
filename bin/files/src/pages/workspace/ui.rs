//! A window-owned workspace. Services finish on Tokio; native owners receive results.
use super::model::Tab;
use super::model::*;
use super::name_form::{NameForm, NamePurpose};
use crate::{
    entities::{
        location::{
            Location,
            places::{Error as PlacesError, Places},
            volumes,
        },
        preferences,
    },
    features::pick_files,
};
use anti_webui::prelude::*;
use gds::{
    app::Gds,
    service::task_manager::{Owner as TaskOwner, Scope},
};
use std::{
    cell::{Cell, RefCell},
    ffi::OsString,
    future::Future,
    path::PathBuf,
    rc::{Rc, Weak},
};

pub struct LaunchOptions {
    pub places: Result<Places, PlacesError>,
    pub start: PathBuf,
    pub home: PathBuf,
    pub picker: Option<pick_files::Options>,
}

#[derive(Clone)]
pub struct Browser(pub(super) Rc<Workspace>);
pub struct WeakBrowser(Weak<Workspace>);
impl Clone for WeakBrowser {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl WeakBrowser {
    pub fn upgrade(&self) -> Option<Browser> {
        self.0
            .upgrade()
            .map(Browser)
            .filter(|browser| browser.0.owner.is_alive())
    }
}

pub(super) struct Workspace {
    pub state: RefCell<State>,
    pub revision: Signal<u64>,
    pub selection: Signal<u64>,
    pub column_widths:
        Signal<std::collections::HashMap<crate::entities::file::columns::Column, f32>>,
    pub pages: Signal<Pages>,
    pub window_width: Signal<f32>,
    pub content_width: Signal<f32>,
    pub address_presentation: Signal<AddressPresentation>,
    pub sidebar_tabs_height: Signal<Option<f32>>,
    pub settings: crate::widgets::settings::View,
    pub sidebar: Signal<bool>,
    pub details: Signal<bool>,
    pub terminal: Signal<bool>,
    pub notice: Signal<Option<String>>,
    pub notifications: gds::ui::notifications::Center,
    pub prompt: Signal<Prompt>,
    pub address: Signal<String>,
    pub filter: Signal<String>,
    pub save_name: Signal<String>,
    pub list: NodeRef,
    pub content_focus: NodeRef,
    pub root_focus: NodeRef,
    pub list_focused: Cell<bool>,
    pub list_focus_visible: Signal<bool>,
    pub settings_node: NodeRef,
    pub address_node: NodeRef,
    pub filter_node: NodeRef,
    pub tabs_node: NodeRef,
    pub locations_node: NodeRef,
    pub volumes_node: NodeRef,
    pub sidebar_region: Signal<Option<super::keyboard::SidebarRegion>>,
    pub sidebar_cursor: Signal<Option<super::keyboard::SidebarRow>>,
    pub sidebar_reveal_volumes: Signal<bool>,
    pub tasks: TaskOwner,
    pub owner: Owner,
    pub window: WindowHandle,
    pub volumes: volumes::Service,
    pub preferences: preferences::Service,
    pub new_window: Rc<dyn Fn(PathBuf)>,
    pub inspections: gds::ui::file_inspection::Cache,
    pub terminals: super::terminals::Dock,
    pub connector: crate::features::connect_server::Connector,
    pub menus: super::menus::Menus,
    pub picker_result: Option<Rc<dyn Fn(Vec<PathBuf>)>>,
}

impl Browser {
    pub fn new(
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
        let inspections = gds::ui::file_inspection::Cache::new(
            (*tasks).clone(),
            preferences::Service::current()
                .snapshot()
                .files
                .preview
                .cache_options(),
        );
        let terminals = super::terminals::Dock::new();
        let preference_service = preferences::Service::current();
        let sidebar_tabs_height = preference_service
            .snapshot()
            .files
            .sidebar
            .tabs_height
            .filter(|height| height.is_finite() && *height > 0.0);
        let notice = signal(notice);
        let notifications = gds::ui::notifications::Center::new();
        effect(move || {
            if let Some(message) = notice.get() {
                untrack(|| notifications.error(message));
            }
        });
        let apply = preference_service.clone();
        let settings =
            crate::widgets::settings::View::new(preference_service.state(), move |preferences| {
                if let Err(error) = apply.update(preferences) {
                    notice.set(Some(error.to_string()));
                }
            });
        let initial_width = 960.0;
        let browser = Self(Rc::new(Workspace {
            details: signal(places.details_visible(location.clone())),
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
            }),
            revision: signal(0),
            selection: signal(0),
            column_widths: signal(Default::default()),
            pages: signal(Pages::default()),
            window_width: signal(initial_width),
            content_width: signal(initial_width - gds::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH),
            address_presentation: signal(AddressPresentation::Breadcrumbs),
            sidebar_tabs_height: signal(sidebar_tabs_height),
            settings,
            sidebar: signal(true),
            terminal: signal(false),
            notice,
            notifications,
            prompt: signal(Prompt::Closed),
            address: signal(location.display_address()),
            filter: signal(String::new()),
            save_name: signal(save_name),
            list: node_ref(),
            content_focus: node_ref(),
            root_focus: node_ref(),
            list_focused: Cell::new(false),
            list_focus_visible: signal(false),
            settings_node: node_ref(),
            address_node: node_ref(),
            filter_node: node_ref(),
            tabs_node: node_ref(),
            locations_node: node_ref(),
            volumes_node: node_ref(),
            sidebar_region: signal(None),
            sidebar_cursor: signal(None),
            sidebar_reveal_volumes: signal(false),
            tasks,
            connector,
            inspections,
            terminals,
            menus: super::menus::Menus::new(),
            picker_result,
            owner,
            window: current_window(),
            volumes,
            preferences: preferences::Service::current(),
            new_window,
        }));
        browser
            .0
            .state
            .borrow()
            .places
            .drain_on_close(&Gds::current().tasks().scope(), browser.0.tasks.token());
        let weak = browser.downgrade();
        on_cleanup(move || {
            if let Some(browser) = weak.0.upgrade() {
                browser.state.borrow().operation.cancel();
                browser.tasks.cancel();
            }
        });
        let close = browser.downgrade();
        on_close_request(move |_| {
            if let Some(browser) = close.upgrade() {
                browser.cancel_picker();
            }
        });
        effect(|| {
            if window_active() {
                crate::shared::lib::clipboard::Clipboard::current().refresh();
            }
        });
        browser.observe_services();
        browser.observe_terminals();
        browser.initialize_picker();
        let ids = browser.read(|state| state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>());
        for id in ids {
            browser.refresh_tab(id);
        }
        browser.focus_content_after_mount();
        browser
    }

    pub(super) fn focus_content_after_mount(&self) {
        if self.pane() == Pane::Files
            && matches!(
                self.0.address_presentation.get_untracked(),
                AddressPresentation::Breadcrumbs
            )
            && self.0.content_focus.logical_rect().is_some()
        {
            self.focus_content();
            return;
        }
        let weak = self.downgrade();
        self.0.owner.with(|| {
            request_frame(move |_| {
                if let Some(browser) = weak.upgrade()
                    && browser.pane() == Pane::Files
                    && matches!(
                        browser.0.address_presentation.get_untracked(),
                        AddressPresentation::Breadcrumbs
                    )
                {
                    browser.focus_content();
                }
            });
        });
    }

    pub(super) fn focus_content(&self) {
        if self.0.content_focus.logical_rect().is_some() {
            self.0.content_focus.focus();
        } else {
            self.0.root_focus.focus();
        }
    }

    pub(super) fn pane(&self) -> Pane {
        self.0.pages.with(Pages::current)
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
        if self.pane() == Pane::Files {
            self.focus_content_after_mount();
        } else {
            let node = self.0.settings_node;
            self.0.owner.with(|| request_frame(move |_| node.focus()));
        }
    }

    pub(super) fn toggle_settings(&self) {
        if self.pane() == Pane::Settings {
            self.back_page();
        } else {
            self.show_settings();
        }
    }

    pub(super) fn show_settings(&self) {
        self.open_page(Pane::Settings);
    }

    pub fn downgrade(&self) -> WeakBrowser {
        WeakBrowser(Rc::downgrade(&self.0))
    }
    pub(super) fn track(&self) {
        self.0.revision.get();
        self.0.selection.get();
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
    pub(super) fn update_selection<R>(&self, edit: impl FnOnce(&mut State) -> R) -> R {
        let result = edit(&mut self.0.state.borrow_mut());
        self.0
            .selection
            .update(|revision| *revision = revision.wrapping_add(1));
        result
    }
    pub(super) fn update<R>(&self, edit: impl FnOnce(&mut State) -> R) -> R {
        let value = edit(&mut self.0.state.borrow_mut());
        self.changed();
        value
    }
    pub fn report_notice(&self, notice: impl Into<String>) {
        self.0.notice.set(Some(notice.into()));
    }
    pub(super) fn report_success(&self, message: impl Into<String>) {
        self.0.notifications.success(message);
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
        self.0.owner.with(|| {
            let (delivery, done) = Task::pending();
            let task = self.0.tasks.spawn_named(name, work);
            task.on_complete(&self.0.tasks, move |result| {
                done.complete(
                    result
                        .map_err(|error| error.to_string())
                        .and_then(std::convert::identity),
                );
            });
            delivery.then(move |result| {
                if let Some(browser) = browser.upgrade() {
                    finished(&browser, result);
                }
            });
        });
    }

    pub(super) fn blocking<T: Send + 'static>(
        &self,
        name: &'static str,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
        finished: impl FnOnce(&Browser, Result<T, String>) + 'static,
    ) {
        let browser = self.downgrade();
        self.0.owner.with(|| {
            let (delivery, done) = Task::pending();
            self.0.tasks.spawn_blocking_named(name, work).on_complete(
                &self.0.tasks,
                move |result| {
                    done.complete(
                        result
                            .map_err(|error| error.to_string())
                            .and_then(std::convert::identity),
                    );
                },
            );
            delivery.then(move |result| {
                if let Some(browser) = browser.upgrade() {
                    finished(&browser, result);
                }
            });
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

    pub(super) fn confirm(&self, title: String, message: String, accept: impl Fn() + 'static) {
        self.confirm_labelled(title, message, gds::i18n::text("dialog-confirm"), accept);
    }
    pub(super) fn confirm_labelled(
        &self,
        title: String,
        message: String,
        primary: String,
        accept: impl Fn() + 'static,
    ) {
        self.0.prompt.set(Prompt::Confirm {
            title,
            message,
            primary,
            accept: Rc::new(accept),
        });
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
        self.0.prompt.set(Prompt::Name {
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
        self.0.prompt.set(Prompt::Name {
            form,
            submit: Rc::new(submit),
            cancel: Some(Rc::new(cancel)),
        });
    }
    pub(super) fn cancel_prompt(&self) {
        let prompt = self.0.prompt.get_untracked();
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
    fn observe_services(&self) {
        let previous = RefCell::new(self.0.preferences.snapshot());
        let browser = self.clone();
        effect(move || {
            let preferences = browser.0.preferences.snapshot();
            let prior = previous.replace(preferences.clone());
            untrack(|| {
                browser.apply_terminal_preferences(&preferences);
                browser
                    .0
                    .inspections
                    .set_options(preferences.files.preview.cache_options());
                if prior.files.show_hidden != preferences.files.show_hidden
                    || prior.files.watching != preferences.files.watching
                    || prior.files.streaming != preferences.files.streaming
                {
                    let ids = browser.read(|state| {
                        state
                            .tabs
                            .iter()
                            .filter(|tab| {
                                matches!(
                                    tab.contents,
                                    Contents::Directory | Contents::Remote { .. }
                                )
                            })
                            .map(|tab| tab.id)
                            .collect::<Vec<_>>()
                    });
                    for id in ids {
                        browser.refresh_tab(id);
                    }
                } else if prior.files.enrichment != preferences.files.enrichment
                    || prior.files.probe_budget != preferences.files.probe_budget
                {
                    let ids = browser
                        .read(|state| state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>());
                    for id in ids {
                        browser.enrich_directory(id);
                    }
                }
                if prior.tags != preferences.tags {
                    let ids = browser.read(|state| {
                        state
                            .tabs
                            .iter()
                            .filter(|tab| matches!(tab.navigation.location(), Location::Tags(_)))
                            .map(|tab| tab.id)
                            .collect::<Vec<_>>()
                    });
                    for id in ids {
                        browser.refresh_tab(id);
                    }
                }
            });
        });
        let browser = self.clone();
        let snapshot = memo({
            let volumes = browser.0.volumes.clone();
            move || volumes.snapshot()
        });
        effect(move || {
            snapshot.with(|_| ());
            untrack(|| {
                let (collections, restored) = browser.read(|state| {
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
                });
                for id in collections {
                    browser.reproject(id);
                }
                if let Some(id) = restored {
                    browser.refresh_tab(id);
                }
            });
        });
        let browser = self.clone();
        effect(move || {
            if let Some(error) = browser.0.preferences.error().get() {
                untrack(|| browser.report_notice(error));
            }
        });
    }
}
