//! Preferences storage for the current application.
//!
//! Every application owns its configuration file; there is no suite-wide
//! settings store. A former suite-wide file is imported once, read-only, so
//! existing preferences survive the move without keeping two writable sources.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use directories::BaseDirs;

use super::model::Preferences;

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

/// Preferences loaded for application start.
pub struct Loaded {
    /// Preferences to apply for this run.
    pub preferences: Preferences,
    /// Legacy shared preferences were read but could not be copied to the
    /// application path. `preferences` still holds those values for this run;
    /// the legacy file is left untouched and the copy is retried next start.
    pub migration: Option<Error>,
}

/// Locate this application's preferences file using the platform's XDG configuration directory.
///
/// # Errors
/// Returns `NoConfigDirectory` if the user's configuration directory is unavailable.
pub fn path() -> Result<PathBuf, Error> {
    let directories = BaseDirs::new().ok_or(Error::NoConfigDirectory)?;
    Ok(directories.config_dir().join("gds/files/settings.toml"))
}

/// Locate the former suite-wide preferences file, used only as an import source.
///
/// # Errors
/// Returns `NoConfigDirectory` if the user's configuration directory is unavailable.
fn legacy_path() -> Result<PathBuf, Error> {
    let directories = BaseDirs::new().ok_or(Error::NoConfigDirectory)?;
    Ok(directories.config_dir().join("gds/settings.toml"))
}

/// Load the preferences that apply at application start.
///
/// The application configuration takes priority. When it does not exist yet, a
/// previously saved suite-wide configuration is read and copied to the
/// application path without replacing or deleting the old file. A malformed
/// legacy file is an error rather than an excuse to write defaults. A copy that
/// still leaves the application path missing keeps the legacy values for this
/// run and reports the failure; a copy that lost to an application configuration
/// created meanwhile uses that configuration for this run.
///
/// # Errors
/// Returns errors for unavailable user directories, unreadable or malformed
/// configuration. A configuration that exists at the application path is never
/// replaced by legacy values.
pub fn load_startup() -> Result<Loaded, Error> {
    load_startup_from(&path()?, &legacy_path()?)
}

/// Save preferences using a same-directory temporary file and atomic replacement.
///
/// # Errors
/// Returns errors for managed symlink/read-only destinations, serialization,
/// directory creation, writing or persistence. The old file is not truncated.
pub fn save(preferences: &Preferences) -> Result<(), Error> {
    save_to(&path()?, preferences)
}

/// # Errors
/// Returns configuration inspection, read or parse errors.
#[cfg(test)]
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
/// Returns inspection, read or parse errors for either path.
fn load_startup_from(application: &Path, legacy: &Path) -> Result<Loaded, Error> {
    if metadata(application)?.is_some() {
        return Ok(Loaded {
            preferences: read(application)?,
            migration: None,
        });
    }
    if metadata(legacy)?.is_none() {
        return Ok(Loaded {
            preferences: Preferences::default(),
            migration: None,
        });
    }
    let preferences = read(legacy)?;
    match write_atomic(application, &preferences, false) {
        Ok(()) => Ok(Loaded {
            preferences,
            migration: None,
        }),
        Err(failure) => import_outcome(application, preferences, failure),
    }
}

/// Decide which preferences this run uses when the import copy did not happen.
///
/// A concurrent instance can create the application path after the initial
/// check, and a configuration manager can create it as a read-only file or a
/// link while the temporary file is written. Such a file holds the newest
/// settings, so it stays authoritative and is never replaced for the sake of the
/// import. Only a still missing application path keeps the legacy values for
/// this run and reports the copy failure.
///
/// # Errors
/// Returns inspection, read or parse errors of an application configuration that
/// appeared during the copy. Unreadable or malformed data is reported instead of
/// being treated as a completed import.
fn import_outcome(
    application: &Path,
    legacy: Preferences,
    failure: Error,
) -> Result<Loaded, Error> {
    if metadata(application)?.is_none() {
        return Ok(Loaded {
            preferences: legacy,
            migration: Some(failure),
        });
    }
    Ok(Loaded {
        preferences: read(application)?,
        migration: None,
    })
}

/// # Errors
/// Returns serialization, destination protection, or filesystem write errors.
fn save_to(path: &Path, preferences: &Preferences) -> Result<(), Error> {
    write_atomic(path, preferences, true)
}

/// Write through a temporary file in the destination directory.
///
/// Without `replace`, an existing file is never overwritten, so concurrent
/// writers cannot clobber each other's configuration.
///
/// # Errors
/// Returns serialization, destination protection, or filesystem write errors.
fn write_atomic(path: &Path, preferences: &Preferences, replace: bool) -> Result<(), Error> {
    let file = gds::persistence::File::new(path.to_owned());
    let expected = if replace { file.read()? } else { None };
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
