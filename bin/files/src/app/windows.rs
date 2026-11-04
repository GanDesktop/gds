//! Application-owned Files windows and desktop request routing.
use crate::entities::{location::places::Places, preferences as configuration};
use crate::pages::workspace::{Browser, LaunchOptions, NewWindowRequested};
use crate::shared::i18n;
use gpui_kit::*;
use std::path::PathBuf;
use tokio::runtime::Handle;

struct Manager {
    _windows: Entity<Windows>,
}
impl Global for Manager {}

pub(super) struct Windows {
    runtime: Handle,
    home: PathBuf,
    windows: Vec<(AnyWindowHandle, WeakEntity<Browser>)>,
}

impl Windows {
    pub(super) fn install(runtime: Handle, home: PathBuf, cx: &mut App) -> Entity<Self> {
        let manager = cx.new(|_| Self {
            runtime,
            home,
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
            cx.new(|cx| Browser::new(launch, window, cx))
        })?;
        if picking {
            let picker_window = window.window_id();
            cx.on_window_closed(move |cx, closed| {
                if closed == picker_window {
                    cx.quit();
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
        let runtime = self.runtime.clone();
        let home = self.home.clone();
        let work = runtime.spawn(Places::load());
        cx.spawn(async move |this, cx| {
            let places = work.await.unwrap_or_else(|error| Err(error.into()));
            if let Err(error) = this.update(cx, |this, cx| {
                let preferences = configuration::current(cx);
                let result = this.open(
                    LaunchOptions {
                        runtime,
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
        use gds::service::desktop::file_manager::Operation;
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
        if command.operation != Operation::NewWindow {
            if let Some((window, browser)) = target {
                Self::apply(window, &browser, command, cx);
                return;
            }
        }
        let start = command
            .targets
            .first()
            .map(|target| target.directory.clone())
            .unwrap_or_else(|| self.home.clone());
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

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
