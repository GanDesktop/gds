//! Unix permission bits for one local file or directory.

use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub(crate) const ACCESS_BITS: u32 = 0o777;
const EXECUTE_BITS: u32 = 0o111;

#[derive(Default)]
pub(crate) struct Report {
    pub succeeded: usize,
    pub failures: Vec<Failure>,
}

pub(crate) struct Failure {
    pub path: PathBuf,
    pub error: Error,
}

impl Report {
    fn record(&mut self, path: &Path, result: Result<(), Error>) {
        match result {
            Ok(()) => self.succeeded += 1,
            Err(error) => self.failures.push(Failure {
                path: path.to_path_buf(),
                error,
            }),
        }
    }
}

/// Apply access bits, optionally walking descendants without following links.
/// Child files keep their execute bits; directories are changed after their children
/// so removing directory access does not prevent the rest of the traversal.
/// Failures are collected while processing the remaining items.
pub(crate) fn apply(path: &Path, access: u32, recursive: bool) -> Report {
    let mut report = Report::default();
    if !recursive {
        report.record(path, save(path, access));
        return report;
    }
    for entry in walkdir::WalkDir::new(path)
        .follow_links(false)
        .follow_root_links(false)
        .contents_first(true)
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                let failed_path = error.path().unwrap_or(path).to_path_buf();
                report.record(&failed_path, Err(io::Error::from(error).into()));
                continue;
            }
        };
        let current = entry.path();
        if entry.file_type().is_symlink() {
            if entry.depth() == 0 {
                report.record(current, Err(Error::Symlink));
            }
            continue;
        }
        let result = if entry.file_type().is_dir() || entry.depth() == 0 {
            save(current, access)
        } else {
            entry
                .metadata()
                .map_err(|error| Error::Filesystem(error.into()))
                .and_then(|metadata| {
                    let mode =
                        (access & !EXECUTE_BITS) | (metadata.permissions().mode() & EXECUTE_BITS);
                    save(current, mode)
                })
        };
        report.record(current, result);
    }
    report
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("symbolic link permissions cannot be edited; open the target instead")]
    Symlink,
    #[error(transparent)]
    Filesystem(#[from] io::Error),
}

/// Read the permission bits without following symbolic links.
///
/// # Errors
/// Reports inaccessible paths and symbolic links.
pub(crate) fn read(path: &Path) -> Result<u32, Error> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Err(Error::Symlink);
    }
    Ok(metadata.permissions().mode())
}

/// Replace only the nine access bits, retaining the current special mode bits.
///
/// # Errors
/// Reports inaccessible paths, symbolic links and permission changes denied by the OS.
pub(crate) fn save(path: &Path, access: u32) -> Result<(), Error> {
    let mode = (read(path)? & !ACCESS_BITS) | (access & ACCESS_BITS);
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}
