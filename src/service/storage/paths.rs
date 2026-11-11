use std::{io, path::Path};

use super::Error;

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
    // Inspect ancestors before creating/opening anything; do not silently follow
    // an existing directory symlink, including outside the private leaf.
    for ancestor in parent.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match tokio::fs::symlink_metadata(ancestor).await {
            Ok(info) if !info.is_dir() || info.file_type().is_symlink() => return Err(private()),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let mut builder = tokio::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        builder.mode(0o700);
    }
    builder.create(parent).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if private_directory
            && tokio::fs::symlink_metadata(parent)
                .await?
                .permissions()
                .mode()
                & 0o077
                != 0
        {
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
        open.mode(0o600).custom_flags(nix::libc::O_NOFOLLOW);
    }
    open.open(path).await?;
    Ok(())
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
    open.mode(0o600).custom_flags(nix::libc::O_NOFOLLOW);
    let file = open.open(path).await?.into_std().await;
    // MigrationSet reads applied IDs before executing migrations. Serialize
    // initialization across processes; closing this file releases the lock.
    let file = tasks
        .spawn_blocking(move || {
            file.lock()?;
            Ok::<_, io::Error>(file)
        })
        .await
        .map_err(io::Error::other)??;
    Ok(file)
}

fn private() -> Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "database files must be private, regular and not symlinks",
    )
    .into()
}
