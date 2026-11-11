mod watching;
pub(crate) use watching::Watching;
mod children;
mod content;
mod details;
mod identity;
mod inspection;

pub use self::children::ChildCount;
pub use self::details::*;

use std::{
    ffi::OsString,
    fs::{self, DirEntry, FileType},
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use self::identity::IdentityCache;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadOptions {
    pub show_hidden: bool,
}

/// Entry type captured with `lstat`; a symlink is never reported as its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    SymbolicLink,
    Other,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: OsString,
    /// Display only; filesystem operations must use `path` or `name`.
    pub display_name: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub is_hidden: bool,
    /// Extra header metadata; empty when the platform or the filesystem cannot
    /// provide it, never a fabricated value.
    pub details: EntryDetails,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningOperation {
    Enumerate,
    Metadata,
    FileType,
    ModifiedTime,
}

#[derive(Debug, Clone)]
pub struct EntryWarning {
    /// The containing directory when enumeration failed before obtaining a name.
    pub path: PathBuf,
    pub operation: WarningOperation,
    pub source: Arc<io::Error>,
}

#[derive(Debug, Clone)]
pub struct DirectorySnapshot {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub warnings: Vec<EntryWarning>,
    pub options: ReadOptions,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
    #[error("cannot determine the working directory: {source}")]
    WorkingDirectory {
        #[source]
        source: io::Error,
    },
    #[error("cannot open directory {path}: {source}")]
    OpenDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Read one local directory. Run this function on a background executor, not in rendering.
///
/// Entries are sorted directories-first, then by their original OS names. Child
/// symlinks are classified without following them, including dangling links.
/// Opening an explicitly requested directory symlink is allowed.
///
/// Owner, group, permission, content details and the bounded direct child count
/// are collected here as well; the content probe reads only a bounded header and
/// fails back to the extension without dropping the entry.
///
/// # Errors
/// Returns an error when the working directory cannot be resolved or the target
/// cannot be opened. Failures after opening are retained in `warnings`; entries
/// with unavailable metadata remain present rather than appearing deleted.
pub fn read_directory(path: &Path, options: ReadOptions) -> Result<DirectorySnapshot, Error> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| Error::WorkingDirectory { source })?
            .join(path)
    };
    let entries = fs::read_dir(&path).map_err(|source| Error::OpenDirectory {
        path: path.clone(),
        source,
    })?;
    Ok(collect_entries(path, entries, options))
}

fn collect_entries(
    path: PathBuf,
    entries: impl Iterator<Item = io::Result<DirEntry>>,
    options: ReadOptions,
) -> DirectorySnapshot {
    let mut snapshot = DirectorySnapshot {
        path,
        entries: Vec::new(),
        warnings: Vec::new(),
        options,
    };
    let mut identities = IdentityCache::new();
    for result in entries {
        match result {
            Ok(entry) => {
                let hidden = entry.file_name().as_encoded_bytes().first() == Some(&b'.');
                if !options.show_hidden && hidden {
                    continue;
                }
                snapshot.entries.push(Entry::from_dir_entry(
                    entry,
                    &mut identities,
                    &mut snapshot.warnings,
                ));
            }
            Err(error) => snapshot.warnings.push(EntryWarning {
                path: snapshot.path.clone(),
                operation: WarningOperation::Enumerate,
                source: error.into(),
            }),
        }
    }
    snapshot.entries.sort_by(|left, right| {
        (right.kind == EntryKind::Directory)
            .cmp(&(left.kind == EntryKind::Directory))
            .then_with(|| left.name.cmp(&right.name))
    });
    snapshot
}

impl Entry {
    fn from_dir_entry(
        entry: DirEntry,
        identities: &mut IdentityCache,
        warnings: &mut Vec<EntryWarning>,
    ) -> Self {
        let path = entry.path();
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                let modified = match metadata.modified() {
                    Ok(modified) => Some(modified),
                    Err(error) => {
                        warnings.push(EntryWarning {
                            path: path.clone(),
                            operation: WarningOperation::ModifiedTime,
                            source: error.into(),
                        });
                        None
                    }
                };
                Self::from_metadata(&path, &metadata, modified, identities)
            }
            Err(error) => {
                warnings.push(EntryWarning {
                    path: path.clone(),
                    operation: WarningOperation::Metadata,
                    source: error.into(),
                });
                // DirEntry may still provide a type captured during enumeration.
                let kind = match entry.file_type() {
                    Ok(file_type) => EntryKind::from(file_type),
                    Err(error) => {
                        warnings.push(EntryWarning {
                            path: path.clone(),
                            operation: WarningOperation::FileType,
                            source: error.into(),
                        });
                        EntryKind::Unknown
                    }
                };
                let name = entry.file_name();
                Self {
                    path,
                    display_name: name.to_string_lossy().into_owned(),
                    is_hidden: name.as_encoded_bytes().starts_with(b"."),
                    name,
                    kind,
                    size: None,
                    modified: None,
                    details: EntryDetails::default(),
                }
            }
        }
    }

    /// Read one item's properties, including a filesystem root.
    ///
    /// # Errors
    /// Reports metadata failures for the requested path.
    pub fn inspect(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        Ok(Self::from_metadata(
            path,
            &metadata,
            metadata.modified().ok(),
            &mut IdentityCache::new(),
        ))
    }

    fn from_metadata(
        path: &Path,
        metadata: &fs::Metadata,
        modified: Option<SystemTime>,
        identities: &mut IdentityCache,
    ) -> Self {
        let name = path.file_name().unwrap_or(path.as_os_str()).to_owned();
        let kind = EntryKind::from(metadata.file_type());
        // Sample access time before any content probe. Unavailable metadata never
        // reaches this constructor, so those entries remain unenriched.
        let mut details = EntryDetails::from_metadata(metadata, identities);
        match kind {
            EntryKind::Directory => details.children = ChildCount::from_path(path),
            EntryKind::File => details.content = ContentType::detect(&name, path),
            EntryKind::SymbolicLink => details.target = Some(LinkTarget::from_path(path)),
            _ => (),
        }
        Self {
            path: path.to_owned(),
            display_name: name.to_string_lossy().into_owned(),
            is_hidden: name.as_encoded_bytes().starts_with(b"."),
            name,
            kind,
            size: (kind == EntryKind::File).then_some(metadata.len()),
            modified,
            details,
        }
    }
}

impl LinkTarget {
    /// Classify a symlink target for display without turning the link into its target.
    fn from_path(path: &Path) -> Self {
        let target_name = fs::read_link(path)
            .ok()
            .and_then(|target| target.file_name().map(|name| name.to_os_string()));
        match fs::metadata(path) {
            Ok(metadata) => {
                let kind = EntryKind::from(metadata.file_type());
                let content = match kind {
                    EntryKind::File => {
                        let name = target_name.as_deref().unwrap_or_else(|| path.as_os_str());
                        ContentType::detect(name, path)
                    }
                    _ => None,
                };
                Self::Resolved { kind, content }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::Broken,
            Err(_) => Self::Unreachable,
        }
    }
}

impl From<FileType> for EntryKind {
    fn from(file_type: FileType) -> Self {
        if file_type.is_symlink() {
            Self::SymbolicLink
        } else if file_type.is_dir() {
            Self::Directory
        } else if file_type.is_file() {
            Self::File
        } else {
            Self::Other
        }
    }
}

#[cfg(test)]
mod tests;
