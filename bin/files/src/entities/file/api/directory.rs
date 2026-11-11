mod watching;
pub(crate) use watching::Watching;
mod children;
mod content;
mod details;
mod identity;
mod inspection;

pub use self::children::ChildCount;
pub use self::details::*;
pub use self::inspection::*;

use std::{
    ffi::OsString,
    fs::{self, DirEntry, FileType},
    io,
    path::{Path, PathBuf},
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
    pub error_kind: io::ErrorKind,
    pub message: String,
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
                snapshot.entries.push(read_entry(
                    entry,
                    hidden,
                    &mut identities,
                    &mut snapshot.warnings,
                ));
            }
            Err(error) => push_warning(
                &mut snapshot.warnings,
                &snapshot.path,
                WarningOperation::Enumerate,
                error,
            ),
        }
    }
    snapshot.entries.sort_by(|left, right| {
        (right.kind == EntryKind::Directory)
            .cmp(&(left.kind == EntryKind::Directory))
            .then_with(|| left.name.cmp(&right.name))
    });
    snapshot
}

fn read_entry(
    entry: DirEntry,
    is_hidden: bool,
    identities: &mut IdentityCache,
    warnings: &mut Vec<EntryWarning>,
) -> Entry {
    let path = entry.path();
    let name = entry.file_name();
    let mut result = Entry {
        path,
        display_name: name.to_string_lossy().into_owned(),
        name,
        kind: EntryKind::Unknown,
        size: None,
        modified: None,
        is_hidden,
        details: EntryDetails::default(),
    };
    let mut metadata_available = false;
    match fs::symlink_metadata(&result.path) {
        Ok(metadata) => {
            metadata_available = true;
            result.kind = entry_kind(metadata.file_type());
            if result.kind == EntryKind::File {
                result.size = Some(metadata.len());
            }
            match metadata.modified() {
                Ok(modified) => result.modified = Some(modified),
                Err(error) => push_warning(
                    warnings,
                    &result.path,
                    WarningOperation::ModifiedTime,
                    error,
                ),
            }
            result.details = EntryDetails::from_metadata(&metadata, identities);
        }
        Err(error) => {
            push_warning(warnings, &result.path, WarningOperation::Metadata, error);
            // DirEntry may still provide a type captured during enumeration.
            match entry.file_type() {
                Ok(file_type) => result.kind = entry_kind(file_type),
                Err(error) => {
                    push_warning(warnings, &result.path, WarningOperation::FileType, error);
                }
            }
        }
    }
    // Enrichment runs after the metadata sample so the recorded access time is
    // the one from before any content probe. A failure here must never fail the
    // listing: the entry keeps its place with whatever details were available.
    // Entries whose own metadata could not be read are left unenriched instead
    // of reporting inferred content for a path that may no longer exist.
    if metadata_available {
        match result.kind {
            EntryKind::Directory => result.details.children = children::count(&result.path),
            EntryKind::File => result.details.content = content::detect(&result.name, &result.path),
            EntryKind::SymbolicLink => result.details.target = Some(link_target(&result.path)),
            _ => {}
        }
    }
    result
}

impl Entry {
    /// Read one item's properties, including a filesystem root.
    ///
    /// # Errors
    /// Reports metadata failures for the requested path.
    pub fn inspect(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        let name = path.file_name().unwrap_or(path.as_os_str()).to_owned();
        let kind = entry_kind(metadata.file_type());
        let mut details = EntryDetails::from_metadata(&metadata, &mut IdentityCache::new());
        match kind {
            EntryKind::Directory => details.children = children::count(path),
            EntryKind::File => details.content = content::detect(&name, path),
            EntryKind::SymbolicLink => details.target = Some(link_target(path)),
            _ => (),
        }
        Ok(Self {
            path: path.to_owned(),
            display_name: name.to_string_lossy().into_owned(),
            is_hidden: name.as_encoded_bytes().starts_with(b"."),
            name,
            kind,
            size: (kind == EntryKind::File).then_some(metadata.len()),
            modified: metadata.modified().ok(),
            details,
        })
    }
}

/// Classify a symlink target for display without turning the link into its target.
fn link_target(path: &Path) -> LinkTarget {
    let target_name = fs::read_link(path)
        .ok()
        .and_then(|target| target.file_name().map(|name| name.to_os_string()));
    match fs::metadata(path) {
        Ok(metadata) => {
            let kind = entry_kind(metadata.file_type());
            let content = match kind {
                EntryKind::File => {
                    let name = target_name.as_deref().unwrap_or_else(|| path.as_os_str());
                    content::detect(name, path)
                }
                _ => None,
            };
            LinkTarget {
                status: LinkStatus::Resolved,
                kind,
                content,
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => LinkTarget {
            status: LinkStatus::Broken,
            kind: EntryKind::Unknown,
            content: None,
        },
        Err(_) => LinkTarget {
            status: LinkStatus::Unreachable,
            kind: EntryKind::Unknown,
            content: None,
        },
    }
}

fn entry_kind(file_type: FileType) -> EntryKind {
    if file_type.is_symlink() {
        EntryKind::SymbolicLink
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

fn push_warning(
    warnings: &mut Vec<EntryWarning>,
    path: &Path,
    operation: WarningOperation,
    error: io::Error,
) {
    warnings.push(EntryWarning {
        path: path.to_path_buf(),
        operation,
        error_kind: error.kind(),
        message: error.to_string(),
    });
}
