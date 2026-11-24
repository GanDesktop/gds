//! Startup configuration resolution and runtime mode selection.

use super::cli;
use crate::{
    entities::{
        location::{Location, scheme},
        preferences as configuration,
    },
    features::pick_files as picker,
};
use anyhow::Context as _;
use directories::BaseDirs;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(super) enum Mode {
    Browser {
        start: PathBuf,
        locations: Vec<Location>,
        new_window: bool,
    },
    Picker {
        start: PathBuf,
        options: picker::Options,
    },
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    Service,
}

impl Mode {
    pub fn keep_alive(&self) -> bool {
        match self {
            Self::Browser { .. } | Self::Picker { .. } => false,
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
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
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
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
            let first = arguments
                .targets
                .into_iter()
                .next()
                .map(Self::parse_picker_path)
                .transpose()?;
            let start = Self::resolve_start(first, suggested, home)?;
            return Ok(Self::Picker { start, options });
        }

        let new_window = arguments.new_window;
        let locations = Self::resolve_targets(arguments.targets)?;
        let start = locations
            .iter()
            .find_map(|loc| loc.local_path().map(Path::to_path_buf))
            .unwrap_or_else(|| home.to_path_buf());
        Ok(Self::Browser {
            start,
            locations,
            new_window,
        })
    }

    fn parse_picker_path(raw: OsString) -> anyhow::Result<PathBuf> {
        if let Some(text) = raw.to_str()
            && scheme::recognizes(text)
            && let Ok(location) = Location::from_uri(text)
            && let Some(path) = location.local_path()
        {
            return Ok(path.to_path_buf());
        }
        Ok(PathBuf::from(raw))
    }

    fn resolve_targets(targets: Vec<OsString>) -> anyhow::Result<Vec<Location>> {
        let mut resolved = Vec::with_capacity(targets.len());
        for raw in targets {
            let location = if let Some(text) = raw.to_str()
                && scheme::recognizes(text)
            {
                Location::from_uri(text).map_err(|error| anyhow::anyhow!(error.to_string()))?
            } else {
                let path = PathBuf::from(raw);
                let abs = if path.is_absolute() {
                    path
                } else {
                    std::env::current_dir()?.join(path)
                };
                Location::from(abs.as_path())
            };
            let normalized = match location {
                Location::Directory(ref raw_path) => {
                    let path = Path::new(raw_path);
                    if path.is_file()
                        && let Some(parent) = path.parent()
                    {
                        Location::from(parent)
                    } else {
                        location
                    }
                }
                other => other,
            };
            if !resolved.contains(&normalized) {
                resolved.push(normalized);
            }
        }
        Ok(resolved)
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;

    #[test]
    fn resolves_multi_uri_targets_normalizes_files_and_honors_new_window() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("folder");
        std::fs::create_dir(&sub).expect("create subfolder");
        let file = sub.join("note.txt");
        std::fs::write(&file, "hello").expect("write file");

        let file_uri = url::Url::from_file_path(&file)
            .expect("file URI")
            .to_string();
        let args = cli::Arguments::try_parse_from([
            "gds-files",
            "--new-window",
            "--",
            "trash:///",
            &file_uri,
            sub.to_str().expect("utf8 path"),
            "recent:///",
        ])
        .expect("parse CLI args");

        let mode = Mode::resolve(args, dir.path()).expect("resolve mode");
        let Mode::Browser {
            start,
            locations,
            new_window,
        } = mode
        else {
            panic!("expected Browser mode");
        };
        assert!(new_window);
        assert_eq!(start, sub);
        assert_eq!(
            locations,
            vec![
                Location::Trash,
                Location::from(sub.as_path()),
                Location::Recent,
            ]
        );
    }

    #[test]
    fn new_window_conflicts_with_picker_mode() {
        let err = cli::Arguments::try_parse_from(["gds-files", "--new-window", "--pick-file"])
            .expect_err("--new-window must conflict with --pick-file");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
}
