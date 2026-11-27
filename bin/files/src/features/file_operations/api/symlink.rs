//! Atomic symbolic-link creation with conflict-safe naming.

use super::super::snapshot::Snapshot;
use super::names;
use std::{
    ffi::{OsStr, OsString},
    io,
    os::unix::fs::symlink,
    path::PathBuf,
};

const MAX_NAME_ATTEMPTS: u32 = 10_000;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("operation cancelled")]
    Cancelled,
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
    #[error(transparent)]
    Name(#[from] names::NameError),
    #[error("refusing to link an invalid path: {path}")]
    InvalidTarget { path: PathBuf },
    #[error("no unused name was found for {name} in {directory}")]
    Exhausted { directory: PathBuf, name: String },
    #[error("cannot create symbolic link {path}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Created {
    pub path: PathBuf,
    pub target: PathBuf,
    pub snapshot: Option<Snapshot>,
}

pub(crate) struct Request {
    pub target: PathBuf,
    pub directory: PathBuf,
    pub link_suffix: String,
}

/// Build a link name inserting `({suffix})` or `({suffix} {index})` before the extension.
pub(crate) fn candidate_name(original: &OsStr, suffix: &str, index: u32) -> OsString {
    let marker = if index == 1 {
        format!(" ({suffix})")
    } else {
        format!(" ({suffix} {index})")
    };
    match names::split_name(original) {
        Some(parts) => {
            let mut name = parts.stem.to_os_string();
            name.push(marker);
            if let Some(mid) = parts.compound_mid {
                name.push(".");
                name.push(mid);
            }
            name.push(".");
            name.push(parts.extension);
            name
        }
        None => {
            let mut name = original.to_os_string();
            name.push(marker);
            name
        }
    }
}

impl Request {
    /// Atomically create a symbolic link pointing to `self.target` inside `self.directory`.
    ///
    /// # Errors
    /// Rejects non-absolute or nameless targets and reports filesystem failures.
    pub fn execute(self) -> Result<Created, Error> {
        if !self.target.is_absolute() {
            return Err(Error::InvalidTarget { path: self.target });
        }
        let base_name = self
            .target
            .file_name()
            .ok_or_else(|| Error::InvalidTarget {
                path: self.target.clone(),
            })?;
        names::validate(base_name)?;

        let suffix = if self.link_suffix.trim().is_empty() {
            "link"
        } else {
            self.link_suffix.trim()
        };

        for index in 1..=MAX_NAME_ATTEMPTS {
            let name = candidate_name(base_name, suffix, index);
            names::validate(&name)?;
            let destination = self.directory.join(&name);
            match symlink(&self.target, &destination) {
                Ok(()) => {
                    let snapshot = Snapshot::capture(&destination).ok();
                    return Ok(Created {
                        path: destination,
                        target: self.target,
                        snapshot,
                    });
                }
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(source) => {
                    return Err(Error::Filesystem {
                        path: destination,
                        source,
                    });
                }
            }
        }
        Err(Error::Exhausted {
            directory: self.directory,
            name: base_name.to_string_lossy().escape_debug().to_string(),
        })
    }

    pub fn spawn(
        self,
        tasks: &gds::service::task_manager::Scope,
        cancel: tokio_util::sync::CancellationToken,
    ) -> gds::service::task_manager::Task<Result<Created, Error>> {
        tasks.spawn_blocking_named("Create symbolic link", move || {
            if cancel.is_cancelled() {
                Err(Error::Cancelled)
            } else {
                self.execute()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn candidate_name_preserves_extensions_and_compound_archives() {
        assert_eq!(
            candidate_name(OsStr::new("report.pdf"), "link", 1),
            OsString::from("report (link).pdf")
        );
        assert_eq!(
            candidate_name(OsStr::new("report.pdf"), "link", 2),
            OsString::from("report (link 2).pdf")
        );
        assert_eq!(
            candidate_name(OsStr::new("archive.tar.gz"), "链接", 1),
            OsString::from("archive (链接).tar.gz")
        );
        assert_eq!(
            candidate_name(OsStr::new("docs"), "link", 1),
            OsString::from("docs (link)")
        );
        assert_eq!(
            candidate_name(OsStr::new(".env"), "link", 2),
            OsString::from(".env (link 2)")
        );
    }

    #[test]
    fn execute_creates_non_overwriting_symlinks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("notes.txt");
        fs::write(&target, b"hello").expect("write target");

        let first = Request {
            target: target.clone(),
            directory: dir.path().to_path_buf(),
            link_suffix: "link".to_owned(),
        }
        .execute()
        .expect("create first symlink");
        assert_eq!(first.path, dir.path().join("notes (link).txt"));
        assert_eq!(fs::read_link(&first.path).expect("read link"), target);
        assert!(first.snapshot.is_some());

        let second = Request {
            target: target.clone(),
            directory: dir.path().to_path_buf(),
            link_suffix: "link".to_owned(),
        }
        .execute()
        .expect("create second symlink");
        assert_eq!(second.path, dir.path().join("notes (link 2).txt"));
        assert_eq!(fs::read_link(&second.path).expect("read link 2"), target);
    }
}
