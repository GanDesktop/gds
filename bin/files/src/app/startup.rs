//! Configure Files, construct resources, start services, and wait for shutdown.
mod config;
#[cfg(target_os = "linux")]
mod desktop;
mod ui;

use crate::app::cli;
use crate::entities::location::places::Storage;
use crate::features::pick_files as picker;
use anyhow::Context as _;
use clap::Parser;
use config::Config;
use std::{process::ExitCode, sync::Arc};

const IO_THREADS: usize = 2;
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
    let resources = Resources::new(&config)?;
    if let Some(application) = ApplicationHandle::start(config, resources)? {
        application.wait();
    }
    Ok(())
}

struct Resources {
    runtime: tokio::runtime::Runtime,
    storage: Arc<Storage>,
}

impl Resources {
    fn new(config: &Config) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(IO_THREADS)
            .thread_name("gds-io")
            .enable_all()
            .build()
            .context("cannot start the application I/O runtime")?;
        let storage = Arc::new(Storage::new(config.state_path.clone()));
        Ok(Self { runtime, storage })
    }
}

/// Owns service lifetimes until the main-thread GUI loop has exited.
struct ApplicationHandle {
    config: Config,
    resources: Resources,
    logs: Option<gds::service::logs::Store>,
    #[cfg(target_os = "linux")]
    desktop: desktop::Desktop,
}

impl ApplicationHandle {
    fn start(config: Config, resources: Resources) -> anyhow::Result<Option<Self>> {
        let logs =
            gds::service::logs::init_tracing("warn,gds=debug,gds_files=debug", config.enable_logs)?;
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            build = env!("GDS_BUILD_TAG"),
            "Files started"
        );
        #[cfg(target_os = "linux")]
        let (config, desktop) = {
            let mut config = config;
            let Some(desktop) = desktop::Desktop::start(&mut config, &resources.runtime)? else {
                return Ok(None);
            };
            (config, desktop)
        };
        Ok(Some(Self {
            config,
            resources,
            logs,
            #[cfg(target_os = "linux")]
            desktop,
        }))
    }
}
