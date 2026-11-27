//! Portal dialogs and file operations used by the workspace.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickMode {
    OpenImage {
        directory: Option<PathBuf>,
    },
    OpenFolder {
        directory: Option<PathBuf>,
    },
    SaveImage {
        suggested_name: String,
        directory: Option<PathBuf>,
    },
}

/// Opens a desktop FileChooser portal dialog in the requested [`PickMode`].
///
/// # Errors
/// Returns an error if the portal request fails or yields a non-local URI.
pub async fn pick_path(title: String, mode: PickMode) -> anyhow::Result<Option<PathBuf>> {
    let request = match mode {
        PickMode::OpenImage { directory } => {
            SelectedFiles::open_file()
                .title(title.as_str())
                .filter(FileFilter::new("Images").mimetype("image/*"))
                .current_folder::<&Path>(directory.as_deref())?
                .send()
                .await?
        }
        PickMode::OpenFolder { directory } => {
            SelectedFiles::open_file()
                .title(title.as_str())
                .directory(true)
                .current_folder::<&Path>(directory.as_deref())?
                .send()
                .await?
        }
        PickMode::SaveImage {
            suggested_name,
            directory,
        } => {
            SelectedFiles::save_file()
                .title(title.as_str())
                .current_name(suggested_name.as_str())
                .current_folder::<&Path>(directory.as_deref())?
                .send()
                .await?
        }
    };
    match request.response() {
        Ok(files) => files
            .uris()
            .first()
            .map(|uri| {
                url::Url::parse(uri.as_str())?
                    .to_file_path()
                    .map_err(|()| anyhow::anyhow!("Expected a local file"))
            })
            .transpose(),
        Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    #[error("invalid file name")]
    InvalidName,
    #[error("a file named {0} already exists")]
    AlreadyExists(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Atomically renames `path` within its parent directory without replacing an existing entry.
///
/// Returns `Ok(None)` if trimmed name is unchanged, `Ok(Some(new_path))` if renamed.
///
/// # Errors
/// Returns [`RenameError::InvalidName`] for empty or path-separating names,
/// [`RenameError::AlreadyExists`] if the destination already exists, or [`RenameError::Io`] on
/// other filesystem failures.
pub fn rename(path: &Path, name: &str) -> Result<Option<PathBuf>, RenameError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || matches!(trimmed, "." | "..")
        || trimmed.contains(['/', '\\', '\0'])
        || Path::new(trimmed).file_name() != Some(OsStr::new(trimmed))
    {
        return Err(RenameError::InvalidName);
    }
    let target = path.with_file_name(trimmed);
    if target == path {
        return Ok(None);
    }
    match rustix::fs::renameat_with(
        rustix::fs::CWD,
        path,
        rustix::fs::CWD,
        &target,
        rustix::fs::RenameFlags::NOREPLACE,
    ) {
        Ok(()) => Ok(Some(target)),
        Err(error) if error == rustix::io::Errno::EXIST => {
            Err(RenameError::AlreadyExists(trimmed.to_owned()))
        }
        Err(error) => {
            let io_error = std::io::Error::from(error);
            if io_error.kind() == std::io::ErrorKind::AlreadyExists {
                Err(RenameError::AlreadyExists(trimmed.to_owned()))
            } else {
                Err(RenameError::Io(io_error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RenameError, rename};

    #[test]
    fn rename_validates_names_detects_unchanged_and_prevents_overwrite() {
        let dir = tempfile::tempdir().expect("temp dir");
        let original = dir.path().join("photo.png");
        let existing = dir.path().join("other.png");
        std::fs::write(&original, b"one").expect("write original");
        std::fs::write(&existing, b"two").expect("write existing");

        assert!(matches!(
            rename(&original, ""),
            Err(RenameError::InvalidName)
        ));
        assert!(matches!(
            rename(&original, "   "),
            Err(RenameError::InvalidName)
        ));
        assert!(matches!(
            rename(&original, "."),
            Err(RenameError::InvalidName)
        ));
        assert!(matches!(
            rename(&original, ".."),
            Err(RenameError::InvalidName)
        ));
        assert!(matches!(
            rename(&original, "sub/name.png"),
            Err(RenameError::InvalidName)
        ));
        assert!(matches!(
            rename(&original, "sub\\name.png"),
            Err(RenameError::InvalidName)
        ));
        assert!(matches!(
            rename(&original, "a\0b.png"),
            Err(RenameError::InvalidName)
        ));

        assert_eq!(
            rename(&original, "  photo.png  ").expect("unchanged rename"),
            None
        );

        assert!(matches!(
            rename(&original, "other.png"),
            Err(RenameError::AlreadyExists(name)) if name == "other.png"
        ));
        assert_eq!(std::fs::read(&existing).expect("read existing"), b"two");

        let renamed = rename(&original, "  renamed.png  ")
            .expect("valid rename")
            .expect("should return new path");
        assert_eq!(renamed, dir.path().join("renamed.png"));
        assert!(!original.exists());
        assert_eq!(std::fs::read(&renamed).expect("read renamed"), b"one");
    }
}
