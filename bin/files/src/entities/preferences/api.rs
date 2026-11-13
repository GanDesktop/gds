//! Preferences storage for the current application.
//!
//! Every application owns its configuration file; there is no suite-wide
//! settings store.

use std::path::{Path, PathBuf};

use directories::BaseDirs;
use gds::persistence::File;

use super::model::Preferences;

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

/// Load the preferences that apply at application start.
///
/// A missing file means the defaults; an existing file is read as-is.
///
/// # Errors
/// Returns errors for unavailable user directories, unreadable or malformed
/// configuration.
pub fn load() -> Result<Preferences, Error> {
    Store::new(path()?).load()
}

pub(super) struct Store {
    file: File,
    expected: Option<Vec<u8>>,
}

impl Store {
    pub(super) fn new(path: PathBuf) -> Self {
        let file = File::new(path);
        let expected = file.read().ok().flatten();
        Self { file, expected }
    }

    fn path(&self) -> &Path {
        self.file.path()
    }

    /// # Errors
    /// Returns configuration read, UTF-8 or TOML parse errors.
    pub(super) fn load(&self) -> Result<Preferences, Error> {
        let Some(bytes) = self.file.read()? else {
            return Ok(Preferences::default());
        };
        let contents = std::str::from_utf8(&bytes).map_err(|source| Error::Utf8 {
            path: self.path().to_owned(),
            source,
        })?;
        toml::from_str(contents).map_err(|source| Error::Parse {
            path: self.path().to_owned(),
            source,
        })
    }

    /// # Errors
    /// Returns serialization, external conflict, destination protection, or filesystem write errors.
    pub(super) fn save(&mut self, preferences: &Preferences) -> Result<(), Error> {
        let contents = toml::to_string_pretty(preferences)?.into_bytes();
        self.file.write(self.expected.as_deref(), &contents)?;
        self.expected = Some(contents);
        Ok(())
    }
}
