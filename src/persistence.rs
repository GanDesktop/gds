//! Atomic application-owned files. Schemas and migrations stay with each application.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("configuration I/O at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("configuration changed externally: {0}")]
    Conflict(PathBuf),
    #[error("configuration is being saved by another writer: {0}")]
    Busy(PathBuf),
    #[error("configuration is managed by a symbolic link: {0}")]
    Symlink(PathBuf),
    #[error("configuration is read-only: {0}")]
    ReadOnly(PathBuf),
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}

#[derive(Clone)]
pub struct File {
    path: PathBuf,
}

impl File {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// # Errors
    /// Reports unreadable files; a missing file is an absent document.
    pub fn read(&self) -> Result<Option<Vec<u8>>, Error> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_error(&self.path, error)),
        }
    }

    /// Replace an unchanged document, or create one that is still absent.
    ///
    /// # Errors
    /// Rejects managed paths, read-only files, concurrent writers and external edits.
    pub fn write(&self, expected: Option<&[u8]>, bytes: &[u8]) -> Result<(), Error> {
        self.writable()?;
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
        let mut lock_path = self.path.as_os_str().to_owned();
        lock_path.push(".lock");
        let lock_path = PathBuf::from(lock_path);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(nix::libc::O_NOFOLLOW);
        }
        let lock = options
            .open(&lock_path)
            .map_err(|error| io_error(&lock_path, error))?;
        lock.try_lock().map_err(|error| match error {
            fs::TryLockError::WouldBlock => Error::Busy(self.path.clone()),
            fs::TryLockError::Error(error) => io_error(&lock_path, error),
        })?;
        let _lock = lock;
        if self.read()?.as_deref() != expected {
            return Err(Error::Conflict(self.path.clone()));
        }
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| io_error(parent, error))?;
        if expected.is_some() {
            let permissions = fs::metadata(&self.path)
                .map_err(|error| io_error(&self.path, error))?
                .permissions();
            temporary
                .as_file()
                .set_permissions(permissions)
                .map_err(|error| io_error(&self.path, error))?;
        }
        temporary
            .write_all(bytes)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|error| io_error(&self.path, error))?;
        self.writable()?;
        if self.read()?.as_deref() != expected {
            return Err(Error::Conflict(self.path.clone()));
        }
        if expected.is_none() {
            temporary.persist_noclobber(&self.path).map_err(|error| {
                if error.error.kind() == io::ErrorKind::AlreadyExists {
                    Error::Conflict(self.path.clone())
                } else {
                    io_error(&self.path, error.error)
                }
            })?;
        } else {
            temporary
                .persist(&self.path)
                .map_err(|error| io_error(&self.path, error.error))?;
        }
        Ok(())
    }

    fn writable(&self) -> Result<(), Error> {
        let mut needs_write_permission = true;
        for path in self
            .path
            .ancestors()
            .filter(|path| !path.as_os_str().is_empty())
        {
            let metadata = match fs::symlink_metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(io_error(path, error)),
            };
            if metadata.file_type().is_symlink() {
                return Err(Error::Symlink(path.into()));
            }
            if needs_write_permission {
                let readonly = metadata.permissions().readonly();
                #[cfg(unix)]
                let readonly = {
                    use std::os::unix::fs::PermissionsExt;
                    readonly || metadata.permissions().mode() & 0o222 == 0
                };
                if readonly {
                    return Err(Error::ReadOnly(path.into()));
                }
                // Only the file and its nearest existing parent need to be writable.
                // Higher ancestors, such as Fedora's /, only need traversal access.
                needs_write_permission = !metadata.is_dir();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
