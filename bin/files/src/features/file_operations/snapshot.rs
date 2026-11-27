//! Filesystem identity and content stamp captured when a file operation commits.

use std::{fs, io, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetKind {
    File,
    Directory,
    Symlink,
    Other,
}

impl TargetKind {
    fn of(file_type: fs::FileType) -> Self {
        if file_type.is_dir() {
            Self::Directory
        } else if file_type.is_symlink() {
            Self::Symlink
        } else if file_type.is_file() {
            Self::File
        } else {
            Self::Other
        }
    }
}

/// Filesystem identity and content stamp captured when an operation commits its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    kind: TargetKind,
    size: u64,
    #[cfg(unix)]
    modified: i64,
    #[cfg(unix)]
    modified_nanos: i64,
    #[cfg(not(unix))]
    modified: Option<std::time::SystemTime>,
}

impl Snapshot {
    /// Capture the current `lstat` stamp of a committed path without following symlinks.
    ///
    /// # Errors
    /// Propagates filesystem metadata lookup failures.
    pub fn capture(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        Ok(Self::from_metadata(&metadata))
    }

    fn from_metadata(metadata: &fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt as _;

        Self {
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            kind: TargetKind::of(metadata.file_type()),
            size: metadata.len(),
            #[cfg(unix)]
            modified: metadata.mtime(),
            #[cfg(unix)]
            modified_nanos: metadata.mtime_nsec(),
            #[cfg(not(unix))]
            modified: metadata.modified().ok(),
        }
    }

    pub(super) fn same_identity(&self, current: &Self) -> bool {
        #[cfg(unix)]
        {
            self.device == current.device
                && self.inode == current.inode
                && self.kind == current.kind
        }
        #[cfg(not(unix))]
        {
            self.kind == current.kind
        }
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub(super) fn matches_directory_dev_ino(&self, dev: u64, ino: u64) -> bool {
        self.kind == TargetKind::Directory && self.device == dev && self.inode == ino
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    pub(super) fn is_directory(&self) -> bool {
        self.kind == TargetKind::Directory
    }
}
