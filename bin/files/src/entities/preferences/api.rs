//! Preferences storage for the current application.
//!
//! Every application owns its configuration file; there is no suite-wide
//! settings store.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use directories::BaseDirs;

use super::model::Preferences;
use gpui_kit::{App, Global};

pub(crate) struct Current(pub Preferences);
impl Global for Current {}

pub(crate) fn current(cx: &App) -> Preferences {
    cx.try_global::<Current>()
        .map(|current| current.0.clone())
        .unwrap_or_default()
}

pub(crate) fn publish(preferences: Preferences, cx: &mut App) {
    if cx
        .try_global::<Current>()
        .is_none_or(|current| current.0 != preferences)
    {
        cx.set_global(Current(preferences));
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not determine the user's configuration directory")]
    NoConfigDirectory,
    #[error("could not inspect configuration {path}: {source}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not read configuration {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
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
    Ok(directories.config_dir().join("gds/files/settings.toml"))
}

/// Load the preferences that apply at application start.
///
/// A missing file means the defaults; an existing file is read as-is.
///
/// # Errors
/// Returns errors for unavailable user directories, unreadable or malformed
/// configuration.
pub fn load() -> Result<Preferences, Error> {
    load_from(&path()?)
}

/// Save preferences using a same-directory temporary file and atomic replacement.
///
/// # Errors
/// Returns errors for managed symlink/read-only destinations, serialization,
/// directory creation, writing or persistence. The old file is not truncated.
pub fn save(preferences: &Preferences, cx: &mut App) -> Result<(), Error> {
    // UI tests use in-memory preferences; storage tests call save_to with temporary paths.
    #[cfg(test)]
    if cx.is_test() {
        publish(preferences.clone(), cx);
        return Ok(());
    }
    save_to(&path()?, preferences)?;
    publish(preferences.clone(), cx);
    Ok(())
}

/// # Errors
/// Returns configuration inspection, read or parse errors.
fn load_from(path: &Path) -> Result<Preferences, Error> {
    if metadata(path)?.is_none() {
        return Ok(Preferences::default());
    }
    read(path)
}

/// # Errors
/// Returns configuration inspection, read or parse errors.
fn read(path: &Path) -> Result<Preferences, Error> {
    let contents = fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_owned(),
        source,
    })?;
    toml::from_str(&contents).map_err(|source| Error::Parse {
        path: path.to_owned(),
        source,
    })
}

/// # Errors
/// Returns serialization, destination protection, or filesystem write errors.
fn save_to(path: &Path, preferences: &Preferences) -> Result<(), Error> {
    let file = gds::persistence::File::new(path.to_owned());
    let expected = file.read()?;
    let contents = toml::to_string_pretty(preferences)?;
    file.write(expected.as_deref(), contents.as_bytes())?;
    Ok(())
}

/// # Errors
/// Returns inspection errors other than a missing file.
fn metadata(path: &Path) -> Result<Option<fs::Metadata>, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Metadata {
            path: path.to_owned(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests;
