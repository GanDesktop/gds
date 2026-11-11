//! Resolve all startup inputs before constructing application resources.
use crate::{app::cli, entities::preferences as configuration, features::pick_files as picker};
use anyhow::Context as _;
use directories::BaseDirs;
use std::path::PathBuf;

pub(super) struct Config {
    pub home: PathBuf,
    pub state_path: PathBuf,
    pub start: PathBuf,
    pub picker: Option<picker::Options>,
    pub preferences: configuration::Preferences,
    pub startup_error: Option<String>,
    pub enable_logs: bool,
    #[cfg(target_os = "linux")]
    pub dbus_service: bool,
}

impl Config {
    pub fn load(arguments: cli::Arguments) -> anyhow::Result<Self> {
        let dirs = BaseDirs::new().context("cannot determine the home directory")?;
        let home = dirs.home_dir().to_path_buf();
        let state_path = dirs
            .state_dir()
            .unwrap_or(dirs.data_local_dir())
            .join("gds/files/state.sqlite");
        let picker_options = if arguments.portal_request {
            Some(picker::Options::from_request(serde_json::from_reader(
                std::io::stdin().lock(),
            )?)?)
        } else {
            arguments.picker()
        };
        let suggested = picker_options
            .as_ref()
            .and_then(|options| options.portal.as_ref())
            .and_then(|request| {
                request
                    .current_file
                    .as_ref()
                    .map(std::path::PathBuf::from)
                    .and_then(|file| file.parent().map(std::path::Path::to_owned))
                    .or_else(|| {
                        request
                            .current_folder
                            .as_ref()
                            .map(std::path::PathBuf::from)
                    })
            })
            .filter(|path| path.is_absolute() && path.is_dir());
        let start = match arguments.directory.or(suggested) {
            Some(path) if path.is_absolute() => path,
            Some(path) => std::env::current_dir()?.join(path),
            None => home.clone(),
        };
        let (preferences, startup_error) = match configuration::load() {
            Ok(preferences) => (preferences, None),
            Err(error) => (
                configuration::Preferences::default(),
                Some(error.to_string()),
            ),
        };
        Ok(Self {
            home,
            state_path,
            start,
            picker: picker_options,
            preferences,
            startup_error,
            enable_logs: arguments.enable_logs,
            #[cfg(target_os = "linux")]
            dbus_service: arguments.dbus_service,
        })
    }

    pub fn report(&mut self, message: String) {
        self.startup_error = Some(match self.startup_error.take() {
            Some(previous) => format!("{previous}\n{message}"),
            None => message,
        });
    }
}
