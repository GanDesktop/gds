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
pub(crate) fn apply_cancellable(
    path: &Path,
    access: u32,
    recursive: bool,
    cancel: &tokio_util::sync::CancellationToken,
) -> Report {
    let mut report = Report::default();
    if cancel.is_cancelled() {
        return report;
    }
    if !recursive {
        report.record(path, save(path, access));
        return report;
    }
    for entry in walkdir::WalkDir::new(path)
        .follow_links(false)
        .follow_root_links(false)
        .contents_first(true)
    {
        if cancel.is_cancelled() {
            break;
        }
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
        let preserve_execute = !entry.file_type().is_dir() && entry.depth() > 0;
        let preserve_mask = if preserve_execute {
            !ACCESS_BITS | EXECUTE_BITS
        } else {
            !ACCESS_BITS
        };
        report.record(current, save_preserving(current, access, preserve_mask));
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

/// Replace only the nine access bits, retaining the current special mode bits,
/// without following symbolic links at any step.
///
/// # Errors
/// Reports inaccessible paths, symbolic links and permission changes denied by the OS.
pub(crate) fn save(path: &Path, access: u32) -> Result<(), Error> {
    save_preserving(path, access, !ACCESS_BITS)
}

fn save_preserving(path: &Path, access: u32, preserve_mask: u32) -> Result<(), Error> {
    use rustix::{
        fs::{CWD, FileType, Mode, OFlags, fstat, openat},
        io::Errno,
    };

    #[cfg(target_os = "linux")]
    let open_flags = OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let open_flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC;

    let fd = match openat(CWD, path, open_flags, Mode::empty()) {
        Ok(fd) => fd,
        Err(Errno::LOOP) => return Err(Error::Symlink),
        Err(error) => return Err(io::Error::from(error).into()),
    };
    let stat = fstat(&fd).map_err(io::Error::from)?;
    if FileType::from_raw_mode(stat.st_mode) == FileType::Symlink {
        return Err(Error::Symlink);
    }
    let writable_mask = ACCESS_BITS & !preserve_mask;
    let mode = (stat.st_mode & preserve_mask) | (access & writable_mask);
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::{AtFlags, chmodat};
        use std::os::fd::AsRawFd;

        let fd_path = format!("/proc/self/fd/{}", fd.as_raw_fd());
        chmodat(CWD, fd_path, Mode::from_raw_mode(mode), AtFlags::empty())
            .map_err(io::Error::from)?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        use rustix::fs::fchmod;

        fchmod(&fd, Mode::from_raw_mode(mode)).map_err(io::Error::from)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
