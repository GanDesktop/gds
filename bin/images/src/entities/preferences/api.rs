//! TOML preference storage backed by `gds::persistence::File`.

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use directories::BaseDirs;
use gds::persistence::File;

use super::model::Preferences;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not determine user configuration directory")]
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

/// Resolves the per-user `settings.toml` path for `GDS Images`.
///
/// # Errors
/// Returns [`Error::NoConfigDirectory`] when `XDG_CONFIG_HOME` / home cannot be resolved.
pub(super) fn path() -> Result<PathBuf, Error> {
    let dirs = BaseDirs::new().ok_or(Error::NoConfigDirectory)?;
    Ok(dirs.config_dir().join("gds/app/images/settings.toml"))
}

pub(super) struct Store {
    file: File,
    expected: Option<Vec<u8>>,
}

pub struct StartupRead {
    pub preferences: Preferences,
    pub(super) store: Store,
    pub notice: Option<String>,
}

/// Reads `settings.toml` exactly once at startup, returning defaults plus a diagnostic notice
/// instead of overwriting corrupt user configuration on disk.
///
/// # Errors
/// Returns configuration directory or I/O read failures.
pub fn read_startup() -> Result<StartupRead, Error> {
    read_startup_from(path()?)
}

pub(crate) fn read_startup_from(path: PathBuf) -> Result<StartupRead, Error> {
    let file = File::new(path.clone());
    let Some(bytes) = file.read()? else {
        return Ok(StartupRead {
            preferences: Preferences::default(),
            store: Store {
                file,
                expected: None,
            },
            notice: None,
        });
    };

    let parsed = std::str::from_utf8(&bytes)
        .map_err(|source| Error::Utf8 {
            path: path.clone(),
            source,
        })
        .and_then(|text| {
            toml::from_str::<Preferences>(text).map_err(|source| Error::Parse { path, source })
        });

    match parsed {
        Ok(preferences) => Ok(StartupRead {
            preferences,
            store: Store {
                file,
                expected: Some(bytes),
            },
            notice: None,
        }),
        Err(error) => Ok(StartupRead {
            preferences: Preferences::default(),
            store: Store {
                file,
                expected: Some(bytes),
            },
            notice: Some(error.to_string()),
        }),
    }
}

impl Store {
    /// # Errors
    /// Returns TOML serialization or atomic persistence errors.
    pub(super) fn save(&mut self, preferences: &Preferences) -> Result<(), Error> {
        let serialized = toml::to_string_pretty(preferences)?.into_bytes();
        self.file.write(self.expected.as_deref(), &serialized)?;
        self.expected = Some(serialized);
        Ok(())
    }
}
