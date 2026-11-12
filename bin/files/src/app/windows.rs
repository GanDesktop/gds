//! Application-owned Files windows and desktop request routing.
use crate::entities::location::places::{Error as PlacesError, Places, Storage};
use crate::features::pick_files as picker;
use crate::pages::workspace::{Browser, LaunchOptions, NewWindowRequested};
use crate::shared::app::Files;
use gds::app::AppExt as _;
use gpui_kit::*;
use std::{path::PathBuf, sync::Arc};

/// Retains `Windows` for the lifetime of the `App` so `NewWindowRequested`
/// subscriptions stay active and startup/desktop handlers can route window
/// operations through the application context.
struct Manager(Entity<Windows>);
impl Global for Manager {}

#[derive(Clone, Copy)]
pub(in crate::app) struct Accessor<C> {
    cx: C,
}

impl<C> Files<C> {
    pub(in crate::app) fn windows(self) -> Accessor<C> {
        Accessor {
            cx: self.into_inner(),
        }
    }
}

impl Accessor<&mut App> {
    pub(in crate::app) fn install(
        &mut self,
        home: PathBuf,
        storage: Arc<Storage>,
        notices: Vec<String>,
    ) {
        assert!(
            !self.cx.has_global::<Manager>(),
            "windows manager already installed"
        );
        let manager = self.cx.new(|_| Windows {
            home,
            storage,
            notices,
            windows: Vec::new(),
        });
        self.cx.set_global(Manager(manager));
    }

    /// Opens the process's initial browser or picker window.
    ///
    /// # Errors
    ///
    /// Returns an error if the application is already shutting down or the
    /// platform window cannot be created.
    pub(in crate::app) fn open_initial(
        &mut self,
        places: Result<Places, PlacesError>,
        start: PathBuf,
        picker: Option<picker::Options>,
    ) -> anyhow::Result<()> {
        let manager = self.cx.global::<Manager>().0.clone();
        manager.update(self.cx, |windows, cx| {
            windows.open_initial(places, start, picker, cx)
        })
    }

    #[cfg(target_os = "linux")]
    pub(in crate::app) fn dispatch_desktop(
        &mut self,
        command: gds::service::desktop::file_manager::Command,
    ) {
        let manager = self.cx.global::<Manager>().0.clone();
        manager.update(self.cx, |windows, cx| windows.desktop_command(command, cx));
    }
}

struct Windows {
    home: PathBuf,
    storage: Arc<Storage>,
    notices: Vec<String>,
    windows: Vec<(AnyWindowHandle, WeakEntity<Browser>)>,
}

impl Windows {
    /// # Errors
    /// Returns an error if the application is shutting down or the platform window cannot be
    /// created.
    fn open_initial(
        &mut self,
        places: Result<Places, PlacesError>,
        start: PathBuf,
        picker: Option<picker::Options>,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let (window, browser) = self.open(
            LaunchOptions {
                places,
                start,
                home: self.home.clone(),
                picker,
            },
            cx,
        )?;
        #[cfg(feature = "files-e2e")]
        if std::env::var_os("GDS_E2E_ARTIFACTS").is_some() {
            crate::pages::workspace::e2e::start(window, browser, cx);
        }
        #[cfg(not(feature = "files-e2e"))]
        let _ = (window, browser);
        Ok(())
    }

    /// # Errors
    /// Returns an error if the application is shutting down or the platform window cannot be
    /// created.
    fn open(
        &mut self,
        launch: LaunchOptions,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<(AnyWindowHandle, Entity<Browser>)> {
        anyhow::ensure!(!cx.gds_ro().tasks().is_shutting_down(), "Files is closing");
        let picking = launch.picker.is_some();
        let options = Self::window_options(launch.picker.as_ref(), cx);
        let (window, browser) = gpui_kit::open_window(options, cx, move |window, cx| {
            Self::configure_window(window, launch.picker.as_ref(), cx);
            cx.new(|cx| Browser::new(launch, window, cx))
        })?;
        // Flush startup warnings onto the process's first window (including
        // delayed `--dbus-service` activation) once `Browser::new` has bound
        // notifications to `window`.
        for notice in self.notices.drain(..) {
            gds::ui::notifications::error(notice, cx);
        }
        if picking {
            let picker_window = window.window_id();
            // `EXIT` defaults to cancelled unless a selection is confirmed, so
            // closing the chooser window requests application quit directly.
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
        let tasks = cx.gds_ro().tasks().scope();
        let home = self.home.clone();
        let storage = self.storage.clone();
        let storage_tasks = tasks.clone();
        let work = tasks.spawn_named("Load window locations", async move {
            storage.load(&storage_tasks).await
        });
        cx.spawn(async move |this, cx| {
            let places = work.await.unwrap_or_else(|error| Err(error.into()));
            if let Err(error) = this.update(cx, |this, cx| {
                if cx.gds_ro().tasks().is_shutting_down() {
                    return;
                }
                let result = this.open(
                    LaunchOptions {
                        places,
                        start,
                        home,
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
    fn desktop_command(&mut self, command: DesktopCommand, cx: &mut Context<Self>) {
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

    fn window_options(picker: Option<&picker::Options>, cx: &App) -> WindowOptions {
        let picking = picker.is_some();
        let dimensions = gds::ui::window::fit_size(gds::ui::window::initial_size(), cx);
        WindowOptions {
            window_min_size: Some(gds::ui::window::minimum_size()),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None, dimensions, cx,
            ))),
            app_id: Some(
                if picking {
                    super::PICKER_APP_ID
                } else {
                    super::APP_ID
                }
                .into(),
            ),
            kind: if picking
                && picker
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
        }
    }

    fn configure_window(window: &mut Window, picker: Option<&picker::Options>, cx: &mut App) {
        let request = picker.and_then(|options| options.portal.as_ref());
        let title = request
            .map(|request| request.title.as_str())
            .filter(|title| !title.is_empty());
        let default_title = cx.gds_ro().i18n().text("app-title");
        window.set_window_title(title.unwrap_or(default_title.as_ref()));
        if let Some(request) = request.filter(|request| !request.parent_window.is_empty())
            && let Err(error) = window.set_external_parent(&request.parent_window)
        {
            tracing::warn!(
                parent = %request.parent_window,
                %error,
                "cannot associate chooser with its parent"
            );
        }
        cx.gds().appearance().theme().attach_observer(window);
    }
}

#[cfg(target_os = "linux")]
type DesktopCommand = gds::service::desktop::file_manager::Command;
#[cfg(not(target_os = "linux"))]
type DesktopCommand = ();
