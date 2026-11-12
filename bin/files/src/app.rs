//! Files application startup and native window lifecycle.

mod cli;
mod config;
#[cfg(target_os = "linux")]
mod desktop;
mod ui;
mod windows;

use crate::entities::location::places::{Error as PlacesError, Places, Storage};
use crate::features::pick_files as picker;
use crate::shared::app::{Files, FilesExt as _};
use anyhow::Context as _;
use clap::Parser;
use config::{Config, Mode};
use gds::app::AppExt as _;
use gds::service::{
    logs::{self, Store},
    task_manager::{Scope, Supervisor},
};
use std::{process::ExitCode, sync::Arc};

pub(super) const APP_ID: &str = "io.github.GanDesktop.GdsFiles";
pub(super) const PICKER_APP_ID: &str = "io.github.GanDesktop.GdsFiles.Picker";
pub(super) const APP_NAME: &str = "GDS Files";

/// Runs the Files process and translates the outcome into a process exit status.
pub(crate) fn run() -> ExitCode {
    let arguments = cli::Arguments::parse();
    let picking = arguments.is_picker();
    match Bootstrap::init(arguments).and_then(Bootstrap::run_event_loop) {
        Ok(()) if picking => ExitCode::from(picker::exit_code()),
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("gds-files: {error:#}");
            if picking {
                // Distinguish startup or stdout serialization failures (exit code 2) from
                // interactive user cancellation (exit code 1, the default picker state).
                picker::mark_failed();
                return ExitCode::from(picker::exit_code());
            }
            ExitCode::FAILURE
        }
    }
}

struct Bootstrap {
    config: Config,
    logs: Option<Store>,
    supervisor: Supervisor,
}

impl Bootstrap {
    /// Resolves configuration, tracing, and the Tokio task supervisor before starting GPUI.
    ///
    /// # Errors
    /// Returns an error if configuration loading, tracing installation, or supervisor creation
    /// fails.
    fn init(arguments: cli::Arguments) -> anyhow::Result<Self> {
        let config = Config::load(arguments)?;
        let logs = logs::init_tracing("warn,gds=debug,gds_files=debug", config.enable_logs)?;
        let supervisor = Supervisor::new(config.enable_logs)
            .context("cannot start the application task supervisor")?;
        Ok(Self {
            config,
            logs,
            supervisor,
        })
    }

    /// Runs the GPUI event loop, driving asynchronous I/O preparation and main-thread service
    /// installation.
    ///
    /// # Errors
    /// Returns an error if single-instance negotiation, UI initialization, or initial window
    /// creation fails.
    fn run_event_loop(self) -> anyhow::Result<()> {
        let Self {
            config,
            logs,
            supervisor,
        } = self;
        // `supervisor` owns the Tokio runtime and must stay alive until the GPUI event loop and
        // task drain have finished.
        let tasks = supervisor.scope();
        let (completed, mut startup) = tokio::sync::oneshot::channel();

        // GPUI constructs `App` inside `run`; running bootstrap as an async task keeps the event
        // loop responsive while the oneshot channel surfaces startup errors after the loop exits.
        gpui_kit::application()
            .with_quit_mode(gpui_kit::QuitMode::Explicit)
            .with_assets(crate::shared::ui::assets::Assets)
            .run(move |cx| {
                cx.spawn(async move |cx| {
                    cx.update(|cx| cx.gds().files().bind_lifecycle(tasks.clone()));

                    let worker = tasks.clone();
                    let io = tasks
                        .spawn_cancellable_named("Start Files services", prepare_io(config, worker))
                        .await;

                    let result = cx.update(move |cx| {
                        // A SIGINT/SIGTERM during startup cancels `prepare_io`; check shutdown
                        // state before unwrapping the cancellation error so signal-driven exit
                        // finishes cleanly.
                        if cx.gds_ro().tasks().is_shutting_down() {
                            return Ok(());
                        }
                        let Some(prepared) = io?? else {
                            cx.gds().tasks().request_quit();
                            return Ok(());
                        };
                        cx.gds().files().mount(prepared, logs)
                    });
                    if let Err(error) = &result {
                        tracing::error!(%error, "Cannot start Files");
                        cx.update(|cx| cx.gds().tasks().request_quit());
                    }
                    let _ = completed.send(result);
                })
                .detach();
            });

        match startup.try_recv() {
            Ok(result) => result,
            // Native quit may terminate the event loop before the startup task sends its result.
            Err(
                tokio::sync::oneshot::error::TryRecvError::Empty
                | tokio::sync::oneshot::error::TryRecvError::Closed,
            ) => Ok(()),
        }
    }
}

/// Asynchronous I/O state prepared on Tokio before installing GPUI services on the main thread.
struct PreparedIo {
    config: Config,
    storage: Arc<Storage>,
    places: Option<Result<Places, PlacesError>>,
    #[cfg(target_os = "linux")]
    desktop: Option<desktop::DesktopIntegration>,
}

/// Prepares single-instance D-Bus ownership and location storage on the Tokio runtime.
///
/// # Errors
/// Returns an error if single-instance D-Bus negotiation or delegating to an existing instance
/// fails.
async fn prepare_io(mut config: Config, tasks: Scope) -> anyhow::Result<Option<PreparedIo>> {
    #[cfg(target_os = "linux")]
    let desktop =
        match desktop::DesktopIntegration::acquire(&config.mode, &mut config.notices).await? {
            desktop::Acquire::Delegated => return Ok(None),
            desktop::Acquire::Continue(desktop) => desktop,
        };

    let storage = Storage::start(config.state_path.clone(), &tasks);
    let places = if config.mode.needs_initial_window() {
        Some(storage.load(&tasks).await)
    } else {
        None
    };
    Ok(Some(PreparedIo {
        config,
        storage,
        places,
        #[cfg(target_os = "linux")]
        desktop,
    }))
}

impl Files<&mut gpui_kit::App> {
    /// Installs GPUI services, opens any initial window, transitions the supervisor to ready, and
    /// starts D-Bus request forwarding.
    ///
    /// # Errors
    /// Returns an error if UI initialization, initial window creation, or desktop request listener
    /// registration fails.
    fn mount(&mut self, prepared: PreparedIo, logs: Option<Store>) -> anyhow::Result<()> {
        let PreparedIo {
            mut config,
            storage,
            places,
            #[cfg(target_os = "linux")]
            desktop,
        } = prepared;

        self.init_ui(config.preferences, logs, &mut config.notices)?;
        self.app()
            .gds()
            .files()
            .windows()
            .install(config.home, storage, config.notices);

        let keep_alive = config.mode.keep_alive();
        match (config.mode, places) {
            (Mode::Browser { start }, Some(places)) => {
                self.app()
                    .gds()
                    .files()
                    .windows()
                    .open_initial(places, start, None)?;
            }
            (Mode::Picker { start, options }, Some(places)) => {
                self.app()
                    .gds()
                    .files()
                    .windows()
                    .open_initial(places, start, Some(options))?;
            }
            _ => {}
        }

        // Transition the supervisor to ready and start serving D-Bus commands only after UI
        // services and any initial window are installed, so idle-quit accounting and incoming
        // requests never observe a partially initialized application.
        self.app().gds().tasks().ready(keep_alive);
        #[cfg(target_os = "linux")]
        if let Some(desktop) = desktop {
            self.listen_desktop(desktop)?;
        }
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            build = env!("GDS_BUILD_TAG"),
            "Files ready"
        );
        Ok(())
    }
}
