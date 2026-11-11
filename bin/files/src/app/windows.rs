//! Application-owned Files windows and desktop request routing.
use crate::entities::{location::places::Storage, preferences::PreferencesExt as _};
use crate::pages::workspace::{Browser, LaunchOptions, NewWindowRequested};
use crate::shared::i18n;
use gds::app::AppExt as _;
use gds::service::task_manager::Scope;
use gpui_kit::*;
use std::{path::PathBuf, sync::Arc};

struct Manager {
    _windows: Entity<Windows>,
}
impl Global for Manager {}

pub(super) struct Windows {
    tasks: Scope,
    home: PathBuf,
    storage: Arc<Storage>,
    windows: Vec<(AnyWindowHandle, WeakEntity<Browser>)>,
}

impl Windows {
    pub(super) fn install(
        tasks: Scope,
        home: PathBuf,
        storage: Arc<Storage>,
        cx: &mut App,
    ) -> Entity<Self> {
        let manager = cx.new(|_| Self {
            tasks,
            home,
            storage,
            windows: Vec::new(),
        });
        cx.set_global(Manager {
            _windows: manager.clone(),
        });
        manager
    }

    pub(super) fn open(
        &mut self,
        launch: LaunchOptions,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<(AnyWindowHandle, Entity<Browser>)> {
        anyhow::ensure!(!cx.gds().tasks().is_shutting_down(), "Files is closing");
        let picking = launch.picker.is_some();
        let dimensions = gds::ui::window::fit_size(gds::ui::window::initial_size(), cx);
        let options = WindowOptions {
            window_min_size: Some(gds::ui::window::minimum_size()),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None, dimensions, cx,
            ))),
            app_id: Some(
                if picking {
                    super::startup::PICKER_APP_ID
                } else {
                    super::startup::APP_ID
                }
                .into(),
            ),
            kind: if picking
                && launch
                    .picker
                    .as_ref()
                    .and_then(|options| options.portal.as_ref())
                    .is_none_or(|request| request.modal)
            {
                WindowKind::Dialog
            } else if picking {
                WindowKind::Floating
            } else {
                WindowKind::Normal
            },
            ..gds::ui::window::window_options()
        };
        let (window, browser) = gpui_kit::open_window(options, cx, move |window, cx| {
            let request = launch
                .picker
                .as_ref()
                .and_then(|options| options.portal.as_ref());
            let title = request
                .map(|request| request.title.as_str())
                .filter(|title| !title.is_empty());
            let default_title = i18n::text("app-title", cx);
            window.set_window_title(title.unwrap_or(default_title.as_ref()));
            if let Some(request) = request.filter(|request| !request.parent_window.is_empty()) {
                if let Err(error) = window.set_external_parent(&request.parent_window) {
                    tracing::warn!(parent = %request.parent_window, %error, "cannot associate chooser with its parent");
                }
            }
            gds::appearance::theme::Observer::attach(window, cx);
            cx.new(|cx| Browser::new(launch, window, cx))
        })?;
        if picking {
            let picker_window = window.window_id();
            cx.on_window_closed(move |cx, closed| {
                if closed == picker_window {
                    cx.gds().tasks().request_quit();
                }
            })
            .detach();
        } else {
            self.windows
                .retain(|(_, browser)| browser.upgrade().is_some());
            self.windows.push((window.into(), browser.downgrade()));
            cx.subscribe(&browser, |this, _, request: &NewWindowRequested, cx| {
                this.request(request.0.clone(), None, cx);
            })
            .detach();
        }
        Ok((window.into(), browser))
    }

    fn request(&mut self, start: PathBuf, command: Option<DesktopCommand>, cx: &mut Context<Self>) {
        let tasks = self.tasks.clone();
        let home = self.home.clone();
        let storage = self.storage.clone();
        let storage_tasks = tasks.clone();
        let work = tasks.spawn_named("Load window locations", async move {
            storage.load(&storage_tasks).await
        });
        cx.spawn(async move |this, cx| {
            let places = work.await.unwrap_or_else(|error| Err(error.into()));
            if let Err(error) = this.update(cx, |this, cx| {
                if cx.gds().tasks().is_shutting_down() {
                    return;
                }
                let preferences = cx.gds().preferences().current();
                let result = this.open(
                    LaunchOptions {
                        tasks,
                        places,
                        start,
                        home,
                        preferences,
                        startup_error: None,
                        picker: None,
                    },
                    cx,
                );
                match result {
                    Ok((window, browser)) => {
                        if let Some(command) = command {
                            Self::apply(window, &browser, command, cx);
                        }
                    }
                    Err(error) => tracing::error!(%error, "Cannot open Files window"),
                }
            }) {
                tracing::warn!(%error, "Files exited before the window opened");
            }
        })
        .detach();
    }

    #[cfg(target_os = "linux")]
    pub(super) fn desktop_command(&mut self, command: DesktopCommand, cx: &mut Context<Self>) {
        use gds::service::desktop::file_manager::Request;
        let active = cx.active_window();
        let target = self
            .windows
            .iter()
            .rev()
            .filter_map(|(window, browser)| browser.upgrade().map(|browser| (*window, browser)))
            .find(|(window, _)| Some(*window) == active)
            .or_else(|| {
                self.windows.iter().rev().find_map(|(window, browser)| {
                    browser.upgrade().map(|browser| (*window, browser))
                })
            });
        if !matches!(&command.request, Request::NewWindow { .. }) {
            if let Some((window, browser)) = target {
                Self::apply(window, &browser, command, cx);
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
        .unwrap_or(&self.home)
        .to_owned();
        self.request(start, Some(command), cx);
    }

    fn apply(
        window: AnyWindowHandle,
        browser: &Entity<Browser>,
        command: DesktopCommand,
        cx: &mut App,
    ) {
        #[cfg(target_os = "linux")]
        if let Err(error) = window.update(cx, |_, window, cx| {
            browser.update(cx, |browser, cx| {
                browser.apply_desktop_command(command, window, cx)
            });
        }) {
            tracing::warn!(%error, "Files window closed before desktop request");
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (window, browser, command, cx);
    }
}

#[cfg(target_os = "linux")]
type DesktopCommand = gds::service::desktop::file_manager::Command;
#[cfg(not(target_os = "linux"))]
type DesktopCommand = ();
