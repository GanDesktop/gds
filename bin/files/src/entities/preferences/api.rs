//! Preferences storage for the current application.
//!
//! Every application owns its configuration file; there is no suite-wide
//! settings store.

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use directories::BaseDirs;
use gds::persistence::File;

use super::model::Preferences;

/// Maximum compare-and-swap reconciliation rounds when concurrent writers race.
const MAX_CONFLICT_RETRIES: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not determine the user's configuration directory")]
    NoConfigDirectory,
    #[error("invalid UTF-8 configuration in {path}: {source}")]
    Utf8 {
        path: PathBuf,
        #[source]
        source: std::str::Utf8Error,
    },
    #[error("invalid configuration in {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not serialize preferences: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error(transparent)]
    Persistence(#[from] gds::persistence::Error),
}

/// Locate this application's preferences file using the platform's XDG configuration directory.
///
/// # Errors
/// Returns `NoConfigDirectory` if the user's configuration directory is unavailable.
pub fn path() -> Result<PathBuf, Error> {
    let directories = BaseDirs::new().ok_or(Error::NoConfigDirectory)?;
    Ok(directories.config_dir().join("gds/app/files/settings.toml"))
}

pub struct StartupRead {
    pub preferences: Preferences,
    pub(super) store: Store,
    pub notice: Option<String>,
}

/// Reads `settings.toml` once at startup, returning defaults plus a diagnostic notice
/// instead of overwriting corrupt or unreadable user configuration on disk.
///
/// # Errors
/// Returns [`Error::NoConfigDirectory`] if the user's configuration directory is unavailable.
pub fn read_startup() -> Result<StartupRead, Error> {
    Ok(read_startup_from(path()?))
}

pub(super) fn read_startup_from(path: PathBuf) -> StartupRead {
    let file = File::new(path);
    let bytes = match file.read() {
        Ok(bytes) => bytes,
        Err(error) => {
            let preferences = Preferences::default();
            return StartupRead {
                store: Store::unreadable(file, preferences.clone()),
                preferences,
                notice: Some(Error::from(error).to_string()),
            };
        }
    };
    let Some(bytes) = bytes else {
        let preferences = Preferences::default();
        return StartupRead {
            store: Store::synced(file, None, preferences.clone(), preferences.clone()),
            preferences,
            notice: None,
        };
    };
    match parse_preferences(file.path(), &bytes) {
        Ok(preferences) => StartupRead {
            store: Store::synced(file, Some(bytes), preferences.clone(), preferences.clone()),
            preferences,
            notice: None,
        },
        Err(error) => {
            let preferences = Preferences::default();
            StartupRead {
                store: Store::unreadable(file, preferences.clone()),
                preferences,
                notice: Some(error.to_string()),
            }
        }
    }
}

fn parse_preferences(path: &Path, bytes: &[u8]) -> Result<Preferences, Error> {
    let contents = std::str::from_utf8(bytes).map_err(|source| Error::Utf8 {
        path: path.to_owned(),
        source,
    })?;
    toml::from_str(contents).map_err(|source| Error::Parse {
        path: path.to_owned(),
        source,
    })
}

enum Baseline {
    /// Disk was absent or parsed cleanly at the last sync point.
    Synced {
        expected: Option<Vec<u8>>,
        disk: Preferences,
        local: Preferences,
    },
    /// Disk could not be read or parsed when this store started; every save
    /// re-checks disk so corrupt files are never overwritten while recovering
    /// automatically once the file is repaired or removed.
    Unreadable { local: Preferences },
}

pub(super) struct Store {
    file: File,
    baseline: Baseline,
}

impl Store {
    fn synced(
        file: File,
        expected: Option<Vec<u8>>,
        disk: Preferences,
        local: Preferences,
    ) -> Self {
        Self {
            file,
            baseline: Baseline::Synced {
                expected,
                disk,
                local,
            },
        }
    }

    fn unreadable(file: File, local: Preferences) -> Self {
        Self {
            file,
            baseline: Baseline::Unreadable { local },
        }
    }

    #[cfg(any(test, feature = "files-e2e"))]
    pub(super) fn for_path(path: PathBuf, local: Preferences) -> Self {
        let file = File::new(path);
        match file.read() {
            Ok(Some(bytes)) => match parse_preferences(file.path(), &bytes) {
                Ok(disk) => Self::synced(file, Some(bytes), disk, local),
                Err(_) => Self::unreadable(file, local),
            },
            Ok(None) => Self::synced(file, None, local.clone(), local),
            Err(_) => Self::unreadable(file, local),
        }
    }

    pub(super) fn set_local_baseline(&mut self, local: Preferences) {
        match &mut self.baseline {
            Baseline::Synced { local: base, .. } | Baseline::Unreadable { local: base } => {
                *base = local;
            }
        }
    }

    fn path(&self) -> &Path {
        self.file.path()
    }

    /// # Errors
    /// Returns serialization, corrupt external file parse/UTF-8, destination
    /// protection, busy, conflict, or filesystem write errors.
    pub(super) fn save(&mut self, preferences: &Preferences) -> Result<(), Error> {
        if let Baseline::Synced {
            expected,
            disk,
            local,
        } = &self.baseline
        {
            let target = Preferences::merge(local, preferences, disk);
            let contents = toml::to_string_pretty(&target)?.into_bytes();
            match self.file.write(expected.as_deref(), &contents) {
                Ok(()) => {
                    self.baseline = Baseline::Synced {
                        expected: Some(contents),
                        disk: target,
                        local: preferences.clone(),
                    };
                    return Ok(());
                }
                Err(gds::persistence::Error::Conflict(_)) => {}
                Err(error) => return Err(error.into()),
            }
        }

        self.reconcile_and_save(preferences)
    }

    fn reconcile_and_save(&mut self, preferences: &Preferences) -> Result<(), Error> {
        let local_base = match &self.baseline {
            Baseline::Synced { local, .. } | Baseline::Unreadable { local } => local.clone(),
        };

        for _ in 0..MAX_CONFLICT_RETRIES {
            let disk_bytes = self.file.read()?;
            let disk_prefs = match &disk_bytes {
                Some(bytes) => parse_preferences(self.path(), bytes)?,
                None => Preferences::default(),
            };
            let merged = Preferences::merge(&local_base, preferences, &disk_prefs);
            let contents = toml::to_string_pretty(&merged)?.into_bytes();
            match self.file.write(disk_bytes.as_deref(), &contents) {
                Ok(()) => {
                    self.baseline = Baseline::Synced {
                        expected: Some(contents),
                        disk: merged,
                        local: preferences.clone(),
                    };
                    return Ok(());
                }
                Err(gds::persistence::Error::Conflict(_)) => continue,
                Err(error) => return Err(error.into()),
            }
        }

        Err(Error::Persistence(gds::persistence::Error::Conflict(
            self.path().to_owned(),
        )))
    }
}
