//! Filesystem execution for new folder.

use super::names;
use std::{ffi::OsString, fs, io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("operation cancelled")]
    Cancelled,
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
    #[error(transparent)]
    Name(#[from] names::NameError),
    #[error("an item named {name} already exists in {directory}")]
    Exists { directory: PathBuf, name: String },
    #[error("cannot create {path}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub(crate) struct Request {
    pub directory: PathBuf,
    pub name: OsString,
}

impl Request {
    /// Create the directory without ever replacing an existing entry; `mkdir`
    /// itself refuses an occupied name, so a target created after the dialog
    /// was confirmed is reported instead of reused.
    ///
    /// # Errors
    /// Rejects invalid leaf names and reports an occupied name or a filesystem
    /// failure.
    pub fn execute(self) -> Result<PathBuf, Error> {
        names::validate(&self.name)?;
        let destination = self.directory.join(&self.name);
        match fs::create_dir(&destination) {
            Ok(()) => Ok(destination),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => Err(Error::Exists {
                directory: self.directory,
                name: self.name.to_string_lossy().escape_debug().to_string(),
            }),
            Err(source) => Err(Error::Filesystem {
                path: destination,
                source,
            }),
        }
    }
}

impl Request {
    pub fn spawn(
        self,
        tasks: &gds::service::task_manager::Scope,
        cancel: tokio_util::sync::CancellationToken,
    ) -> gds::service::task_manager::Task<Result<PathBuf, Error>> {
        tasks.spawn_blocking_named("Create folder", move || {
            if cancel.is_cancelled() {
                Err(Error::Cancelled)
            } else {
                self.execute()
            }
        })
    }
}
