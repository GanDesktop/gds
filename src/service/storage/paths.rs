use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

use super::Error;

const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
const LOCK_POLL: Duration = Duration::from_millis(10);

pub(super) async fn prepare(path: &Path, private_directory: bool) -> Result<(), Error> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "database requires a parent directory",
            )
        })?;
    // Ancestors above `parent` may be symlinks (such as `/var -> private/var`
    // or `~/.local/state -> /persist/...`) provided any existing entry resolves
    // to a directory.
    if let Some(above) = parent.parent() {
        for ancestor in above.ancestors() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            check_ancestor(ancestor).await?;
        }
    }
    // Keep strict no-symlink enforcement on the immediate leaf directory `parent`.
    match tokio::fs::symlink_metadata(parent).await {
        Ok(info) if !info.is_dir() || info.file_type().is_symlink() => return Err(private()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut builder = tokio::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        builder.mode(0o700);
    }
    builder.create(parent).await?;
    let parent_info = tokio::fs::symlink_metadata(parent).await?;
    if !parent_info.is_dir() || parent_info.file_type().is_symlink() {
        return Err(private());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if private_directory && parent_info.permissions().mode() & 0o077 != 0 {
            return Err(private());
        }
    }
    check_file(path).await?;
    // SQLite opens these itself; reject preexisting nonprivate files/symlinks.
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        check_file(Path::new(&name)).await?;
    }
    let mut open = tokio::fs::OpenOptions::new();
    open.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        open.mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC);
    }
    open.open(path).await.map_err(map_open_error)?;
    Ok(())
}

async fn check_ancestor(ancestor: &Path) -> Result<(), Error> {
    match tokio::fs::metadata(ancestor).await {
        Ok(info) if !info.is_dir() => Err(private()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match tokio::fs::symlink_metadata(ancestor).await {
                Ok(info) if info.file_type().is_symlink() || !info.is_dir() => Err(private()),
                Ok(_) => Ok(()),
                Err(link_error) if link_error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(link_error) => Err(link_error.into()),
            }
        }
        Err(error) => {
            #[cfg(unix)]
            if error.raw_os_error() == Some(nix::libc::ENOTDIR) {
                return Err(private());
            }
            Err(error.into())
        }
    }
}

fn map_open_error(error: io::Error) -> Error {
    #[cfg(unix)]
    if error.raw_os_error() == Some(nix::libc::ELOOP) {
        return private();
    }
    error.into()
}

async fn check_file(path: &Path) -> Result<(), Error> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(info) => {
            if !info.is_file() || info.file_type().is_symlink() {
                return Err(private());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                if info.permissions().mode() & 0o077 != 0 || info.nlink() != 1 {
                    return Err(private());
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(super) fn acquire_lock(
    file: std::fs::File,
    timeout: Duration,
    is_cancelled: impl Fn() -> bool,
) -> io::Result<std::fs::File> {
    let deadline = Instant::now() + timeout;
    loop {
        if is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "database initialization lock cancelled",
            ));
        }
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out waiting for database initialization lock",
                    ));
                }
                std::thread::sleep(LOCK_POLL);
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
    }
}

pub(super) async fn lock(
    tasks: &crate::service::task_manager::Scope,
    path: &Path,
) -> Result<std::fs::File, Error> {
    let mut name = path.as_os_str().to_owned();
    name.push(".lock");
    let path = Path::new(&name);
    check_file(path).await?;
    let mut open = tokio::fs::OpenOptions::new();
    open.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    open.mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC);
    let file = open
        .open(path)
        .await
        .map_err(map_open_error)?
        .into_std()
        .await;
    // MigrationSet reads applied IDs before executing migrations. Serialize
    // initialization across processes; closing this file releases the lock.
    let cancel = tasks.token();
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "database initialization lock cancelled",
        )
        .into());
    }
    let file = tasks
        .spawn_blocking_named("Lock database initialization", move || {
            acquire_lock(file, LOCK_TIMEOUT, || cancel.is_cancelled())
        })
        .await
        .map_err(|error| {
            if matches!(error, crate::service::task_manager::Error::Closed(_)) {
                io::Error::new(io::ErrorKind::Interrupted, error)
            } else {
                io::Error::other(error)
            }
        })??;
    Ok(file)
}

fn private() -> Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "database files must be private, regular and not symlinks",
    )
    .into()
}
