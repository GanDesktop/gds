//! Startup order: configuration, logging, task supervisor, I/O services, UI, requests.
mod config;
#[cfg(target_os = "linux")]
mod desktop;
mod ui;

use crate::app::cli;
use crate::entities::location::places::Storage;
use crate::features::pick_files as picker;
use anyhow::Context as _;
use clap::Parser;
use config::{Config, Mode};
use gds::app::AppExt as _;
use std::{process::ExitCode, sync::Arc};
use ui::FilesExt as _;

pub(super) const APP_ID: &str = "io.github.GanDesktop.GdsFiles";
pub(super) const PICKER_APP_ID: &str = "io.github.GanDesktop.GdsFiles.Picker";

/// # Errors
/// Reports invalid arguments or unavailable user/current directories.
pub(crate) fn run() -> anyhow::Result<ExitCode> {
    let arguments = cli::Arguments::parse();
    let picking = arguments.is_picker();
    match launch(arguments) {
        Ok(()) if picking => Ok(ExitCode::from(picker::exit_code())),
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(error) => {
            eprintln!("gds-files: {error:#}");
            if picking {
                picker::mark_failed();
                return Ok(ExitCode::from(picker::exit_code()));
            }
            Ok(ExitCode::FAILURE)
        }
    }
}

fn launch(arguments: cli::Arguments) -> anyhow::Result<()> {
    let config = Config::load(arguments)?;
    let logs =
        gds::service::logs::init_tracing("warn,gds=debug,gds_files=debug", config.enable_logs)?;
    let supervisor = gds::service::task_manager::Supervisor::new(config.enable_logs)
        .context("cannot start the application task supervisor")?;
    let application = Application {
        config,
        logs,
        tasks: supervisor.scope(),
    };
    let (completed, mut startup) = tokio::sync::oneshot::channel();

    // GPUI supplies App inside run; asynchronous startup keeps this event loop responsive.
    gpui_kit::application()
        .with_assets(crate::shared::ui::assets::Assets)
        .run(move |cx| {
            cx.spawn(async move |cx| {
                let result = application.start(cx).await;
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
        // Native quit may end the event loop before startup completes.
        Err(
            tokio::sync::oneshot::error::TryRecvError::Empty
            | tokio::sync::oneshot::error::TryRecvError::Closed,
        ) => Ok(()),
    }
}

struct Application {
    config: Config,
    logs: Option<gds::service::logs::Store>,
    tasks: gds::service::task_manager::Scope,
}

impl Application {
    async fn start(self, cx: &mut gpui_kit::AsyncApp) -> anyhow::Result<()> {
        let Self {
            config,
            logs,
            tasks,
        } = self;
        cx.update(|cx| {
            cx.gds().tasks().init(tasks.clone());
            #[cfg(unix)]
            cx.gds()
                .tasks()
                .signals(|cx| cx.gds().tasks().request_quit());
            cx.on_window_closed(|cx, _| cx.gds().tasks().quit_when_idle())
                .detach();
        });

        let worker = tasks.clone();
        let services = tasks
            .spawn_cancellable(Services::start(config, worker))
            .await;
        cx.update(move |cx| {
            if cx.gds().tasks().is_shutting_down() {
                return Ok(());
            }
            let Some(services) = services?? else {
                cx.gds().tasks().request_quit();
                return Ok(());
            };
            services.start_ui(logs, cx)
        })
    }
}

/// I/O services prepared before the application installs its UI and accepts requests.
struct Services {
    config: Config,
    storage: Arc<Storage>,
    places:
        Result<crate::entities::location::places::Places, crate::entities::location::places::Error>,
    #[cfg(target_os = "linux")]
    desktop: desktop::DesktopIntegration,
}

impl Services {
    async fn start(
        mut config: Config,
        tasks: gds::service::task_manager::Scope,
    ) -> anyhow::Result<Option<Self>> {
        #[cfg(target_os = "linux")]
        let Some(desktop) = desktop::DesktopIntegration::start(&mut config).await? else {
            return Ok(None);
        };
        let storage = Storage::start(config.state_path.clone(), &tasks);
        let places = storage.load(&tasks).await;
        Ok(Some(Self {
            config,
            storage,
            places,
            #[cfg(target_os = "linux")]
            desktop,
        }))
    }

    fn start_ui(
        mut self,
        logs: Option<gds::service::logs::Store>,
        cx: &mut gpui_kit::App,
    ) -> anyhow::Result<()> {
        cx.gds().files().initialize(&mut self.config, logs)?;
        let tasks = cx.gds().tasks().scope();
        let windows = super::windows::Windows::install(
            tasks.clone(),
            self.config.home.clone(),
            self.storage,
            cx,
        );
        let keep_alive = self.config.mode.keep_alive();
        if !keep_alive {
            let Config {
                home,
                start,
                preferences,
                startup_error,
                picker,
                ..
            } = self.config;
            let (window, browser) = windows.update(cx, |windows, cx| {
                windows.open(
                    crate::pages::workspace::LaunchOptions {
                        tasks,
                        places: self.places,
                        start,
                        home,
                        preferences,
                        startup_error,
                        picker,
                    },
                    cx,
                )
            })?;
            #[cfg(feature = "files-e2e")]
            if std::env::var_os("GDS_E2E_ARTIFACTS").is_some() {
                crate::pages::workspace::e2e::start(window, browser.clone(), cx);
            }
            let _ = (window, browser);
        }

        cx.gds().tasks().ready(keep_alive);
        #[cfg(target_os = "linux")]
        self.desktop.listen(windows, cx)?;
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            build = env!("GDS_BUILD_TAG"),
            "Files ready"
        );
        Ok(())
    }
}
