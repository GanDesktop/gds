//! Filesystem execution for rename.

use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NameError {
    #[error("the new name is empty")]
    Empty,
    #[error("the new name contains a path separator")]
    Separator,
    #[error("the new name is a reserved directory name")]
    Reserved,
    #[error("the new name is unchanged")]
    Unchanged,
    #[error("the new name contains an invalid character")]
    Invalid,
}

impl NameError {
    pub fn label_id(self) -> &'static str {
        match self {
            Self::Empty => "rename-error-empty",
            Self::Separator => "rename-error-separator",
            Self::Reserved => "rename-error-reserved",
            Self::Unchanged => "rename-error-unchanged",
            Self::Invalid => "rename-error-invalid",
        }
    }
}

/// Check a typed leaf name without touching the filesystem.
///
/// The comparison is byte-exact: leading and trailing spaces are legal names
/// and are never trimmed implicitly.
pub(crate) fn validate_name(name: &str, current: &OsStr) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.contains('/') || (cfg!(windows) && name.contains('\\')) {
        return Err(NameError::Separator);
    }
    if name == "." || name == ".." {
        return Err(NameError::Reserved);
    }
    if name.contains('\0') {
        return Err(NameError::Invalid);
    }
    if OsStr::new(name) == current {
        return Err(NameError::Unchanged);
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("operation cancelled")]
    Cancelled,
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
    #[error(transparent)]
    Name(#[from] NameError),
    #[error("cannot rename a path without a parent and file name: {path}")]
    InvalidTarget { path: PathBuf },
    #[error("an item named {name} already exists in {parent}")]
    Exists { parent: PathBuf, name: String },
    #[error("cannot rename {path}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[cfg(not(target_os = "linux"))]
    #[error("safe no-replace rename is not available on this platform: {path}")]
    Unsupported { path: PathBuf },
}

#[derive(Debug, Clone)]
pub(crate) struct Request {
    pub source: PathBuf,
    pub name: OsString,
}

impl Request {
    /// Resolve the destination and rename without replacing an existing entry.
    ///
    /// # Errors
    /// Rejects invalid leaf names, a missing destination parent, an existing
    /// target, and failures reported by the filesystem.
    pub(crate) fn execute(&self) -> Result<PathBuf, Error> {
        let current = self
            .source
            .file_name()
            .ok_or_else(|| Error::InvalidTarget {
                path: self.source.clone(),
            })?;
        validate_name(&self.name.to_string_lossy(), current)?;
        let parent = self.source.parent().ok_or_else(|| Error::InvalidTarget {
            path: self.source.clone(),
        })?;
        let destination = parent.join(&self.name);
        if destination == self.source {
            return Err(Error::Name(NameError::Unchanged));
        }
        ensure_absent(&destination)?;
        rename_no_replace(&self.source, &destination)?;
        Ok(destination)
    }
}

fn ensure_absent(destination: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(destination) {
        Ok(_) => Err(exists(destination)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Filesystem {
            path: destination.to_path_buf(),
            source,
        }),
    }
}

fn exists(destination: &Path) -> Error {
    Error::Exists {
        parent: destination
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf),
        name: destination
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
    }
}

#[cfg(target_os = "linux")]
fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), Error> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    match renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE) {
        Ok(()) => Ok(()),
        Err(error) if error == rustix::io::Errno::EXIST => Err(exists(destination)),
        // Refuse rather than fall back to a check-then-rename sequence: that
        // sequence can replace a destination created between the two calls.
        Err(error) => Err(Error::Filesystem {
            path: source.to_path_buf(),
            source: error.into(),
        }),
    }
}

#[cfg(not(target_os = "linux"))]
fn rename_no_replace(source: &Path, _destination: &Path) -> Result<(), Error> {
    // No portable no-replace rename is wired on this target. A plain rename
    // could overwrite an existing entry, so refuse explicitly until a native
    // API is added here.
    Err(Error::Unsupported {
        path: source.to_path_buf(),
    })
}

impl Request {
    pub fn spawn(
        self,
        tasks: &gds::service::task_manager::Scope,
        cancel: tokio_util::sync::CancellationToken,
    ) -> gds::service::task_manager::Task<Result<PathBuf, Error>> {
        tasks.spawn_blocking_named("Rename file", move || {
            if cancel.is_cancelled() {
                Err(Error::Cancelled)
            } else {
                self.execute()
            }
        })
    }
}
