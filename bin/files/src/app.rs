//! Files startup and native `zgui` application lifetime.

mod cli;
mod config;
#[cfg(target_os = "linux")]
mod desktop;
mod ui;
mod windows;

use std::{cell::RefCell, process::ExitCode, rc::Rc, sync::Arc};

use anyhow::Context as _;
use clap::Parser;
use config::{Config, Mode};
use gds::{
    app::Gds,
    service::{
        logs::{self, Store},
        task_manager::{Scope, Supervisor},
    },
};
use tokio::sync::mpsc;
use zgui::prelude::*;
use zgui::reactive::on_cleanup_local;

use crate::{
    entities::location::places::{Error as PlacesError, Places, Storage},
    features::pick_files as picker,
};

pub(super) const APP_ID: &str = "io.github.GanDesktop.GdsFiles";
pub(super) const PICKER_APP_ID: &str = "io.github.GanDesktop.GdsFiles.Picker";
pub(super) const APP_NAME: &str = "Files";

pub(crate) fn run() -> ExitCode {
    #[cfg(feature = "files-e2e")]
    if matches!(
        std::env::var("GDS_E2E_MODE").as_deref(),
        Ok("driven" | "gallery")
    ) {
        return crate::pages::workspace::e2e::run();
    }
    let arguments = cli::Arguments::parse();
    let picking = arguments.is_picker();
    match Bootstrap::init(arguments).and_then(Bootstrap::run_event_loop) {
        Ok(Some(outcome)) => ExitCode::from(outcome),
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("gds-files: {error:#}");
            if picking {
                ExitCode::from(picker::Outcome::Failed)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

struct Bootstrap {
    config: Config,
    logs: Option<Store>,
    supervisor: Supervisor,
}

impl Bootstrap {
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

    async fn prepare_io(mut config: Config, tasks: Scope) -> anyhow::Result<Option<PreparedIo>> {
        #[cfg(target_os = "linux")]
        let desktop =
            match desktop::DesktopIntegration::acquire(&config.mode, &mut config.notices).await? {
                desktop::Acquire::Delegated => return Ok(None),
                desktop::Acquire::Continue(desktop) => desktop,
            };
        let storage = Storage::start(config.state_path.clone(), &tasks);
        let places = if config.mode.needs_initial_window() {
            let (places, ()) = tokio::join!(
                storage.load(&tasks),
                config.preferences.appearance.prime_session()
            );
            Some(places)
        } else {
            config.preferences.appearance.prime_session().await;
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

    fn run_event_loop(self) -> anyhow::Result<Option<picker::Outcome>> {
        let Self {
            mut config,
            logs,
            supervisor,
        } = self;
        let outcome = match &mut config.mode {
            Mode::Picker { options, .. } => Some(options.bind_outcome()),
            _ => None,
        };
        let app_id = if outcome.is_some() {
            PICKER_APP_ID
        } else {
            APP_ID
        };
        let tasks = supervisor.scope();
        let startup_error: Rc<RefCell<Option<anyhow::Error>>> = Rc::new(RefCell::new(None));
        let error_slot = startup_error.clone();

        let result = zgui::app()
            .with_application_id(app_id)
            .without_primary_window()
            .with_exit_policy(ExitPolicy::Explicit)
            .with_context(move || {
                let app = Gds::install_process_with_logs(tasks.clone(), logs);
                Self::bind_signals(&tasks);
                let mount_tasks = tasks.clone();
                app.tasks().spawn_ui(
                    "Start Files services",
                    Self::prepare_io(config, tasks.clone()),
                    move |prepared| {
                        if mount_tasks.token().is_cancelled() {
                            return;
                        }
                        let result = (|| {
                            let Some(prepared) = prepared?? else {
                                if let Some(windows) = try_use_windows() {
                                    windows.quit();
                                }
                                return Ok(());
                            };
                            prepared.mount(&mount_tasks)
                        })();
                        if let Err(error) = result {
                            tracing::error!(%error, "Cannot start Files");
                            *error_slot.borrow_mut() = Some(error);
                            if let Some(windows) = try_use_windows() {
                                windows.quit();
                            }
                        }
                    },
                );
            })
            .run(|| ());

        supervisor.shutdown();
        result.map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if let Some(error) = startup_error.borrow_mut().take() {
            return Err(error);
        }
        Ok(outcome.map(|slot| slot.get()))
    }

    fn bind_signals(tasks: &Scope) {
        #[cfg(unix)]
        {
            let (tx, rx) = mpsc::channel(1);
            zgui::tokio::spawn_receiver(rx, |()| {
                if let Some(windows) = try_use_windows() {
                    windows.quit();
                }
            });
            let cancel = tasks.token();
            let watcher = tasks.spawn_named("Listen for termination signals", async move {
                let mut terminate =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
                let mut interrupt =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
                enum Event {
                    Cancelled,
                    Terminate,
                }
                let event = tokio::select! {
                    _ = cancel.cancelled() => Event::Cancelled,
                    _ = terminate.recv() => Event::Terminate,
                    _ = interrupt.recv() => Event::Terminate,
                };
                if matches!(event, Event::Terminate) {
                    let _ = tx.send(()).await;
                }
                Ok::<_, std::io::Error>(())
            });
            zgui::task::spawn_local(async move {
                if let Err(error) = watcher
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|result| result.map_err(|error| error.to_string()))
                {
                    tracing::error!(%error, "cannot listen for termination signals");
                }
            });
        }
    }
}

struct PreparedIo {
    config: Config,
    storage: Arc<Storage>,
    places: Option<Result<Places, PlacesError>>,
    #[cfg(target_os = "linux")]
    desktop: Option<desktop::DesktopIntegration>,
}

impl PreparedIo {
    fn mount(self, tasks: &Scope) -> anyhow::Result<()> {
        let Self {
            mut config,
            storage,
            places,
            #[cfg(target_os = "linux")]
            desktop,
        } = self;
        ui::Initializer::new(Gds::current()).run(tasks, config.preferences, &mut config.notices)?;
        let windows = windows::Host::new(
            tasks.clone(),
            config.home,
            storage,
            config.notices,
            config.mode.keep_alive(),
        );
        match (config.mode, places) {
            (Mode::Browser { start }, Some(places)) => windows.open_initial(places, start, None),
            (Mode::Picker { start, options }, Some(places)) => {
                windows.open_initial(places, start, Some(options))
            }
            _ => {}
        }
        #[cfg(target_os = "linux")]
        if let Some(desktop) = desktop {
            desktop.listen(tasks, windows.clone());
        }
        on_cleanup_local(move || drop(windows));
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            build = env!("GDS_BUILD_TAG"),
            "Files ready"
        );
        Ok(())
    }
}
