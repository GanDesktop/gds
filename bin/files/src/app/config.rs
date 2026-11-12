//! Startup configuration resolution and runtime mode selection.

use super::cli;
use crate::{entities::preferences as configuration, features::pick_files as picker};
use anyhow::Context as _;
use directories::BaseDirs;
use std::path::{Path, PathBuf};

pub(super) enum Mode {
    Browser {
        start: PathBuf,
    },
    Picker {
        start: PathBuf,
        options: picker::Options,
    },
    #[cfg(target_os = "linux")]
    Service,
}

impl Mode {
    pub fn keep_alive(&self) -> bool {
        match self {
            Self::Browser { .. } | Self::Picker { .. } => false,
            #[cfg(target_os = "linux")]
            Self::Service => true,
        }
    }

    pub fn needs_initial_window(&self) -> bool {
        !self.keep_alive()
    }

    /// # Errors
    /// Returns an error if a `--portal-request` payload on stdin cannot be parsed or the starting
    /// directory cannot be resolved against the current working directory.
    fn resolve(arguments: cli::Arguments, home: &Path) -> anyhow::Result<Self> {
        #[cfg(target_os = "linux")]
        if arguments.dbus_service {
            return Ok(Self::Service);
        }

        let picker_options = if arguments.portal_request {
            Some(picker::Options::from_request(serde_json::from_reader(
                std::io::stdin().lock(),
            )?)?)
        } else {
            arguments.picker()
        };

        if let Some(options) = picker_options {
            let suggested = options
                .portal
                .as_ref()
                .and_then(|request| {
                    request
                        .current_file
                        .as_ref()
                        .map(PathBuf::from)
                        .and_then(|file| file.parent().map(Path::to_owned))
                        .or_else(|| request.current_folder.as_ref().map(PathBuf::from))
                })
                .filter(|path| path.is_absolute() && path.is_dir());
            let start = Self::resolve_start(arguments.directory, suggested, home)?;
            return Ok(Self::Picker { start, options });
        }

        let start = Self::resolve_start(arguments.directory, None, home)?;
        Ok(Self::Browser { start })
    }

    /// # Errors
    /// Returns an error if `directory` is relative and the current working directory cannot be
    /// determined.
    fn resolve_start(
        directory: Option<PathBuf>,
        suggested: Option<PathBuf>,
        home: &Path,
    ) -> anyhow::Result<PathBuf> {
        match directory.or(suggested) {
            Some(path) if path.is_absolute() => Ok(path),
            Some(path) => Ok(std::env::current_dir()?.join(path)),
            None => Ok(home.to_path_buf()),
        }
    }
}

pub(super) struct Config {
    pub home: PathBuf,
    pub state_path: PathBuf,
    pub preferences: configuration::Preferences,
    pub notices: Vec<String>,
    pub enable_logs: bool,
    pub mode: Mode,
}

impl Config {
    /// Resolves filesystem paths, user preferences, and launch mode from CLI arguments.
    ///
    /// # Errors
    /// Returns an error if the user's home or current working directory cannot be determined, or
    /// if a `--portal-request` payload on stdin is invalid.
    pub fn load(arguments: cli::Arguments) -> anyhow::Result<Self> {
        let dirs = BaseDirs::new().context("cannot determine the home directory")?;
        let home = dirs.home_dir().to_path_buf();
        let state_path = dirs
            .state_dir()
            .unwrap_or(dirs.data_local_dir())
            .join("gds/files/state.sqlite");
        let enable_logs = arguments.enable_logs;
        let mode = Mode::resolve(arguments, &home)?;

        let mut notices = Vec::new();
        let preferences = match configuration::load() {
            Ok(preferences) => preferences,
            Err(error) => {
                notices.push(error.to_string());
                configuration::Preferences::default()
            }
        };

        Ok(Self {
            home,
            state_path,
            preferences,
            notices,
            enable_logs,
            mode,
        })
    }
}
