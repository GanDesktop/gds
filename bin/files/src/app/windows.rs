//! Application-owned native windows and desktop request routing.
use crate::{
    entities::location::{
        places::{Error as PlacesError, Places, Storage},
        volumes,
    },
    features::pick_files as picker,
    pages::workspace::{Browser, LaunchOptions, WeakBrowser},
};
use anti_webui::prelude::*;
use gds::{
    app::Gds,
    service::{
        desktop::file_manager::{Command, Request},
        task_manager::Scope,
    },
};
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};

#[derive(Clone)]
pub(super) struct Host(Rc<Windows>);
struct Windows {
    tasks: Scope,
    home: PathBuf,
    storage: Arc<Storage>,
    volumes: volumes::Service,
    keep_alive: bool,
    startup_token: RefCell<Option<String>>,
    state: RefCell<State>,
}
struct State {
    notices: Vec<String>,
    windows: Vec<OpenWindow>,
    pending: usize,
}
#[derive(Clone)]
struct OpenWindow {
    handle: WindowHandle,
    owner: Owner,
    browser: WeakBrowser,
}
impl OpenWindow {
    fn browser(&self) -> Option<Browser> {
        self.browser.upgrade()
    }
    fn active(&self) -> bool {
        self.owner.is_alive() && self.owner.with(window_active)
    }
    fn apply(&self, command: Command) {
        if let Some(browser) = self.browser() {
            self.owner.with(|| browser.apply_desktop_command(command));
        }
    }
}

impl Host {
    pub fn new(
        tasks: Scope,
        home: PathBuf,
        storage: Arc<Storage>,
        notices: Vec<String>,
        keep_alive: bool,
    ) -> Self {
        let volumes = volumes::Service::new(tasks.clone());
        Self(Rc::new(Windows {
            tasks,
            home,
            storage,
            volumes,
            keep_alive,
            startup_token: RefCell::new(
                std::env::var("XDG_ACTIVATION_TOKEN")
                    .ok()
                    .filter(|token| !token.is_empty()),
            ),
            state: RefCell::new(State {
                notices,
                windows: Vec::new(),
                pending: 0,
            }),
        }))
    }
    pub fn open_initial(
        &self,
        places: Result<Places, PlacesError>,
        start: PathBuf,
        picker: Option<picker::Options>,
    ) {
        self.open(
            LaunchOptions {
                places,
                start,
                home: self.0.home.clone(),
                picker,
            },
            None,
        );
    }
    fn open(&self, launch: LaunchOptions, command: Option<Command>) {
        if self.0.tasks.token().is_cancelled() {
            return;
        }
        let portal = launch
            .picker
            .as_ref()
            .and_then(|options| options.portal.as_ref());
        let title = portal
            .map(|request| request.title.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| gds::i18n::text("app-title"));
        let parent = portal.map(|request| (request.parent_window.clone(), request.modal));
        let picking = launch.picker.is_some();
        let startup_token = self.0.startup_token.borrow_mut().take();
        let host = self.clone();
        let window_host = self.clone();
        self.0.state.borrow_mut().pending += 1;
        let options = gds::ui::window::workspace_options()
            .title(title)
            .dialog(picking)
            .top_level();
        open_window(options, move || {
            let window = current_window();
            let id = window.id();
            let owner = Owner::current().expect("window builder has a reactive owner");
            let request_host = host.clone();
            let browser = Browser::new(
                launch,
                host.0.volumes.clone(),
                Rc::new(move |start| request_host.request(start, None)),
            );
            let notices = {
                let mut state = host.0.state.borrow_mut();
                state.pending -= 1;
                state.windows.push(OpenWindow {
                    handle: window,
                    owner,
                    browser: browser.downgrade(),
                });
                std::mem::take(&mut state.notices)
            };
            for notice in notices {
                browser.report_notice(notice);
            }
            if let Some((parent, modal)) = parent.filter(|(parent, _)| !parent.is_empty()) {
                match parent.parse() {
                    Ok(parent) => {
                        let browser = browser.downgrade();
                        set_external_parent(Some(parent), modal).then(move |result| {
                            if let Err(error) = result
                                && let Some(browser) = browser.upgrade()
                            {
                                browser.report_notice(format!(
                                    "Cannot associate chooser with its parent: {error}"
                                ));
                            }
                        });
                    }
                    Err(error) => browser.report_notice(format!("Invalid parent window: {error}")),
                }
            }
            if let Some(command) = command {
                browser.apply_desktop_command(command);
            } else if let Some(token) = startup_token {
                activate_window(token).then(|result| {
                    if let Err(error) = result {
                        tracing::warn!(%error, "cannot submit startup activation");
                    }
                });
            }
            on_cleanup(move || window_host.closed(id, picking));
            browser.view()
        });
    }
    fn closed(&self, id: WindowId, picking: bool) {
        let idle = {
            let mut state = self.0.state.borrow_mut();
            state
                .windows
                .retain(|window| window.handle.id() != id && window.handle.is_open());
            state.windows.is_empty() && state.pending == 0
        };
        if picking || (idle && !self.0.keep_alive) {
            quit();
        }
    }
    fn request(&self, start: PathBuf, command: Option<Command>) {
        self.0.state.borrow_mut().pending += 1;
        let host = self.clone();
        let tasks = self.0.tasks.clone();
        let storage = self.0.storage.clone();
        Gds::current().with(|| {
            Gds::current()
                .tasks()
                .spawn("Load window locations", async move {
                    storage.load(&tasks).await
                })
                .then(move |result| {
                    host.0.state.borrow_mut().pending -= 1;
                    let places = result.unwrap_or_else(|error| Err(error.into()));
                    host.open(
                        LaunchOptions {
                            places,
                            start,
                            home: host.0.home.clone(),
                            picker: None,
                        },
                        command,
                    );
                })
        });
    }
    pub fn dispatch_desktop(&self, command: Command) {
        if !matches!(command.request, Request::NewWindow { .. }) {
            let candidates = self.0.state.borrow().windows.clone();
            if let Some(window) = candidates
                .iter()
                .rev()
                .find(|window| window.active())
                .or_else(|| {
                    candidates
                        .iter()
                        .rev()
                        .find(|window| window.browser().is_some())
                })
            {
                window.apply(command);
                return;
            }
        }
        let start = match &command.request {
            Request::NewWindow { directory } => Some(directory.as_path()),
            Request::ShowFolders { directories } => directories.first().map(|path| path.as_path()),
            Request::ShowItems { paths } | Request::ShowProperties { paths } => {
                paths.first().map(|path| path.parent().unwrap_or(path))
            }
        }
        .unwrap_or(&self.0.home)
        .to_owned();
        self.request(start, Some(command));
    }
    pub fn report_notice(&self, message: String) {
        let candidate = self
            .0
            .state
            .borrow()
            .windows
            .iter()
            .rev()
            .find(|window| window.browser().is_some())
            .cloned();
        if let Some(window) = candidate
            && let Some(browser) = window.browser()
        {
            window.owner.with(|| browser.report_notice(message));
        } else {
            self.0.state.borrow_mut().notices.push(message);
        }
    }
}
