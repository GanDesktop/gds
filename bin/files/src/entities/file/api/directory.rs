mod watching;
pub(crate) use watching::Watching;
mod children;
mod content;
mod details;
mod enrich;
mod identity;
mod inspection;

pub use self::children::ChildCount;
pub use self::details::*;
pub use self::enrich::*;

use std::{
    ffi::{OsStr, OsString},
    fs::{self, DirEntry, FileType},
    io,
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use tokio_util::sync::CancellationToken;

use self::identity::IdentityCache;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadOptions {
    pub show_hidden: bool,
}

/// Elapsed milliseconds before emitting the first progressive directory snapshot.
pub const DEFAULT_STREAM_INITIAL_DELAY_MS: u64 = 150;

/// Minimum milliseconds between subsequent progressive directory snapshots.
pub const DEFAULT_STREAM_INTERVAL_MS: u64 = 250;

/// Minimum collected entries before emitting a progressive snapshot (`0` disables streaming).
pub const DEFAULT_STREAM_MIN_ENTRIES: usize = 200;

/// Time and item-count gates for progressive directory snapshots on slow storage (e.g. USB HDDs or FUSE mounts).
///
/// Scans that finish within `initial_delay` emit zero intermediate snapshots, preserving atomic
/// single-shot transitions on fast local SSDs. Setting `min_entries = 0` disables progressive emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamOptions {
    pub initial_delay: Duration,
    pub interval: Duration,
    pub min_entries: usize,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(DEFAULT_STREAM_INITIAL_DELAY_MS),
            interval: Duration::from_millis(DEFAULT_STREAM_INTERVAL_MS),
            min_entries: DEFAULT_STREAM_MIN_ENTRIES,
        }
    }
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

/// Shared immutable directory entry with O(1) cloning across snapshots and table rows.
#[derive(Debug, Clone)]
pub struct Entry(Arc<EntryData>);

#[derive(Debug, Clone)]
pub struct EntryData {
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
    /// Pre-lowercased display name for non-ASCII entries so filter matching avoids allocations.
    search_name: Option<Box<str>>,
    children_probed: bool,
    content_probed: bool,
    target_probed: bool,
}

impl Deref for Entry {
    type Target = EntryData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        // Internal probe bookkeeping and cached search text do not affect visible entry equality.
        Arc::ptr_eq(&self.0, &other.0)
            || (self.path == other.path
                && self.name == other.name
                && self.display_name == other.display_name
                && self.kind == other.kind
                && self.size == other.size
                && self.modified == other.modified
                && self.is_hidden == other.is_hidden
                && self.details == other.details)
    }
}

impl Eq for Entry {}

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
    pub entries: Arc<[Entry]>,
    pub warnings: Vec<EntryWarning>,
    pub options: ReadOptions,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
    #[error("directory read cancelled")]
    Cancelled,
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

impl Error {
    pub fn is_cancelled(&self) -> bool {
        match self {
            Self::Cancelled => true,
            Self::Task(gds::service::task_manager::Error::Closed(_)) => true,
            Self::Task(gds::service::task_manager::Error::Join(error)) => error.is_cancelled(),
            _ => false,
        }
    }
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
#[cfg(test)]
pub fn read_directory(path: &Path, options: ReadOptions) -> Result<DirectorySnapshot, Error> {
    let cancel = CancellationToken::new();
    read_directory_cancellable(path, options, &cancel)
}

/// Read one local directory with full detail and periodic cancellation checkpoints.
///
/// # Errors
/// Returns [`Error::Cancelled`] when `cancel` is triggered, or an open/working-directory
/// error when the target directory cannot be opened.
#[cfg(test)]
pub fn read_directory_cancellable(
    path: &Path,
    options: ReadOptions,
    cancel: &CancellationToken,
) -> Result<DirectorySnapshot, Error> {
    DirectorySnapshot::read(path, options, cancel, true, None)
}

/// Read the base entries of one local directory without child-count, file-header, or
/// symlink-target probes so the initial listing renders without secondary I/O stalls.
///
/// # Errors
/// Returns [`Error::Cancelled`] when `cancel` is triggered, or an open/working-directory
/// error when the target directory cannot be opened.
pub fn read_directory_base(
    path: &Path,
    options: ReadOptions,
    cancel: &CancellationToken,
) -> Result<DirectorySnapshot, Error> {
    DirectorySnapshot::read(path, options, cancel, false, None)
}

/// Read the base entries of one local directory, emitting time-gated progressive snapshots
/// through `on_progress` when enumeration exceeds [`StreamOptions::initial_delay`] and
/// [`StreamOptions::min_entries`].
///
/// # Errors
/// Returns [`Error::Cancelled`] when `cancel` is triggered, or an open/working-directory
/// error when the target directory cannot be opened.
pub fn read_directory_base_streaming(
    path: &Path,
    options: ReadOptions,
    stream: StreamOptions,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(Arc<DirectorySnapshot>),
) -> Result<DirectorySnapshot, Error> {
    let stream_hook = (stream.min_entries > 0).then_some((
        stream,
        &mut on_progress as &mut dyn FnMut(Arc<DirectorySnapshot>),
    ));
    DirectorySnapshot::read(path, options, cancel, false, stream_hook)
}

impl DirectorySnapshot {
    /// Look up an entry by path in `O(log N)` using the snapshot's canonical sort order.
    pub fn entry_for_path(&self, path: &Path) -> Option<&Entry> {
        let name = path.file_name().unwrap_or(path.as_os_str());
        for is_directory in [false, true] {
            if let Ok(index) = self.entries.binary_search_by(|entry| {
                is_directory
                    .cmp(&(entry.kind == EntryKind::Directory))
                    .then_with(|| entry.name.as_os_str().cmp(name))
                    .then_with(|| entry.path.as_os_str().cmp(path.as_os_str()))
            }) {
                return self.entries.get(index);
            }
        }
        None
    }

    fn read(
        path: &Path,
        options: ReadOptions,
        cancel: &CancellationToken,
        full: bool,
        stream: Option<(StreamOptions, &mut dyn FnMut(Arc<Self>))>,
    ) -> Result<Self, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
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
        Self::collect(path, entries, options, cancel, full, stream)
    }

    fn collect(
        path: PathBuf,
        entries: impl Iterator<Item = io::Result<DirEntry>>,
        options: ReadOptions,
        cancel: &CancellationToken,
        full: bool,
        mut stream: Option<(StreamOptions, &mut dyn FnMut(Arc<Self>))>,
    ) -> Result<Self, Error> {
        let mut collected = Vec::new();
        let mut warnings = Vec::new();
        let mut identities = IdentityCache::new();
        let mut next_emit_at = stream
            .as_ref()
            .map(|(opts, _)| Instant::now() + opts.initial_delay);
        let mut last_emitted_len = 0usize;
        for result in entries {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            match result {
                Ok(entry) => {
                    let hidden = entry.file_name().as_encoded_bytes().first() == Some(&b'.');
                    if !options.show_hidden && hidden {
                        continue;
                    }
                    // Emit before `lstat` on the next unhidden entry so a partial snapshot is
                    // never emitted redundantly after the final directory item.
                    if let (Some((opts, on_progress)), Some(deadline)) =
                        (stream.as_mut(), next_emit_at)
                        && collected.len() >= opts.min_entries
                        && collected.len() > last_emitted_len
                        && Instant::now() >= deadline
                    {
                        collected.sort_by(Entry::canonical_order);
                        on_progress(Arc::new(Self {
                            path: path.clone(),
                            entries: collected.as_slice().into(),
                            warnings: Vec::new(),
                            options,
                        }));
                        last_emitted_len = collected.len();
                        next_emit_at = Some(Instant::now() + opts.interval);
                    }
                    let item = if full {
                        Entry::from_dir_entry(entry, &mut identities, &mut warnings)
                    } else {
                        Entry::from_dir_entry_base(entry, &mut identities, &mut warnings)
                    };
                    collected.push(item);
                }
                Err(error) => warnings.push(EntryWarning {
                    path: path.clone(),
                    operation: WarningOperation::Enumerate,
                    source: error.into(),
                }),
            }
        }
        collected.sort_by(Entry::canonical_order);
        Ok(Self {
            path,
            entries: collected.into(),
            warnings,
            options,
        })
    }
}

impl Entry {
    #[cfg(test)]
    pub fn new(
        path: PathBuf,
        name: OsString,
        kind: EntryKind,
        size: Option<u64>,
        modified: Option<SystemTime>,
        details: EntryDetails,
    ) -> Self {
        let children_probed = details.children.is_some();
        let content_probed = details.content.is_some();
        let target_probed = details.target.is_some();
        Self::from_parts(
            path,
            name,
            kind,
            size,
            modified,
            details,
            children_probed,
            content_probed,
            target_probed,
        )
    }

    /// Match `display_name` against a pre-lowercased substring filter without
    /// per-keystroke allocations.
    pub fn matches_filter(&self, lowercase_filter: &str) -> bool {
        if lowercase_filter.is_empty() {
            return true;
        }
        if self.display_name.is_ascii() && lowercase_filter.is_ascii() {
            self.display_name
                .as_bytes()
                .windows(lowercase_filter.len())
                .any(|window| window.eq_ignore_ascii_case(lowercase_filter.as_bytes()))
        } else {
            self.search_name
                .as_deref()
                .unwrap_or(&self.display_name)
                .contains(lowercase_filter)
        }
    }

    pub(super) fn canonical_order(left: &Self, right: &Self) -> std::cmp::Ordering {
        (right.kind == EntryKind::Directory)
            .cmp(&(left.kind == EntryKind::Directory))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path.as_os_str().cmp(right.path.as_os_str()))
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        path: PathBuf,
        name: OsString,
        kind: EntryKind,
        size: Option<u64>,
        modified: Option<SystemTime>,
        details: EntryDetails,
        children_probed: bool,
        content_probed: bool,
        target_probed: bool,
    ) -> Self {
        let display_name = name.to_string_lossy().into_owned();
        let search_name =
            (!display_name.is_ascii()).then(|| display_name.to_lowercase().into_boxed_str());
        let is_hidden = name.as_encoded_bytes().starts_with(b".");
        Self(Arc::new(EntryData {
            path,
            name,
            display_name,
            kind,
            size,
            modified,
            is_hidden,
            details,
            search_name,
            children_probed,
            content_probed,
            target_probed,
        }))
    }

    fn from_dir_entry(
        entry: DirEntry,
        identities: &mut IdentityCache,
        warnings: &mut Vec<EntryWarning>,
    ) -> Self {
        Self::read_dir_entry(entry, identities, warnings, true)
    }

    fn from_dir_entry_base(
        entry: DirEntry,
        identities: &mut IdentityCache,
        warnings: &mut Vec<EntryWarning>,
    ) -> Self {
        Self::read_dir_entry(entry, identities, warnings, false)
    }

    fn read_dir_entry(
        entry: DirEntry,
        identities: &mut IdentityCache,
        warnings: &mut Vec<EntryWarning>,
        full: bool,
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
                if full {
                    Self::from_metadata(&path, &metadata, modified, identities)
                } else {
                    Self::from_base_metadata(&path, &metadata, modified, identities)
                }
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
                Self::from_parts(
                    path,
                    name,
                    kind,
                    None,
                    None,
                    EntryDetails::default(),
                    true,
                    true,
                    true,
                )
            }
        }
    }

    /// Read one item's properties, including a filesystem root.
    ///
    /// # Errors
    /// Reports metadata failures for the requested path.
    pub fn inspect(path: &Path) -> io::Result<Self> {
        Self::inspect_with_identities(path, &mut IdentityCache::new())
    }

    pub(super) fn inspect_with_identities(
        path: &Path,
        identities: &mut IdentityCache,
    ) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        Ok(Self::from_metadata(
            path,
            &metadata,
            metadata.modified().ok(),
            identities,
        ))
    }

    /// Read one item's base metadata without child-count, file-header, or
    /// symlink-target probes.
    ///
    /// # Errors
    /// Reports metadata failures for the requested path.
    pub(crate) fn inspect_base_with_identities(
        path: &Path,
        identities: &mut IdentityCache,
    ) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        Ok(Self::from_base_metadata(
            path,
            &metadata,
            metadata.modified().ok(),
            identities,
        ))
    }

    /// Preserve already-probed secondary metadata from `previous` when the entry's
    /// identity, kind, size, modification time, and metadata change time still match.
    pub(crate) fn with_reused_probes(mut self, previous: &Self) -> Self {
        self.reuse_probes(previous);
        self
    }

    pub(crate) fn reuse_probes(&mut self, previous: &Self) {
        if self.kind == EntryKind::SymbolicLink
            || (self.modified.is_none() && self.details.changed.is_none())
            || self.path != previous.path
            || self.name != previous.name
            || self.display_name != previous.display_name
            || self.kind != previous.kind
            || self.size != previous.size
            || self.modified != previous.modified
            || self.is_hidden != previous.is_hidden
            || self.details.changed != previous.details.changed
            || self.details.created != previous.details.created
            || self.details.owner != previous.details.owner
            || self.details.group != previous.details.group
            || self.details.permissions != previous.details.permissions
            || self.details.read_only != previous.details.read_only
        {
            return;
        }
        if self.details.accessed == previous.details.accessed {
            *self = previous.clone();
            return;
        }
        let data = Arc::make_mut(&mut self.0);
        match data.kind {
            EntryKind::Directory => {
                data.details.children = previous.details.children;
                data.children_probed = previous.children_probed;
            }
            EntryKind::File => {
                data.details.content = previous.details.content.clone();
                data.content_probed = previous.content_probed;
            }
            EntryKind::SymbolicLink | EntryKind::Other | EntryKind::Unknown => (),
        }
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
        Self::from_parts(
            path.to_owned(),
            name,
            kind,
            (kind == EntryKind::File).then_some(metadata.len()),
            modified,
            details,
            kind == EntryKind::Directory,
            matches!(kind, EntryKind::File | EntryKind::SymbolicLink),
            kind == EntryKind::SymbolicLink,
        )
    }

    fn from_base_metadata(
        path: &Path,
        metadata: &fs::Metadata,
        modified: Option<SystemTime>,
        identities: &mut IdentityCache,
    ) -> Self {
        let name = path.file_name().unwrap_or(path.as_os_str()).to_owned();
        let kind = EntryKind::from(metadata.file_type());
        let mut details = EntryDetails::from_metadata(metadata, identities);
        if kind == EntryKind::File {
            details.content = ContentType::from_filename(&name);
        }
        let content_probed = details.content.is_some();
        Self::from_parts(
            path.to_owned(),
            name,
            kind,
            (kind == EntryKind::File).then_some(metadata.len()),
            modified,
            details,
            false,
            content_probed,
            false,
        )
    }
}

impl ContentType {
    /// Detect the content type strictly from filename patterns in memory without disk I/O.
    pub(crate) fn from_filename(name: &OsStr) -> Option<Self> {
        Self::detect_from_name(name)
    }
}

impl LinkTarget {
    /// Classify a symlink target for display without turning the link into its target.
    pub(super) fn from_path(path: &Path) -> Self {
        Self::inspect(path, true).0
    }

    /// Classify a symlink target without reading the target file's header bytes.
    /// Returns the target state and whether its content classification is complete.
    pub(crate) fn from_path_base(path: &Path) -> (Self, bool) {
        Self::inspect(path, false)
    }

    fn inspect(path: &Path, sniff_header: bool) -> (Self, bool) {
        let target_name = fs::read_link(path)
            .ok()
            .and_then(|target| target.file_name().map(|name| name.to_os_string()));
        match fs::metadata(path) {
            Ok(metadata) => {
                let kind = EntryKind::from(metadata.file_type());
                let (content, content_probed) = match kind {
                    EntryKind::File => {
                        let name = target_name.as_deref().unwrap_or_else(|| path.as_os_str());
                        if sniff_header {
                            (ContentType::detect(name, path), true)
                        } else {
                            let content = ContentType::from_filename(name);
                            let probed = content.is_some();
                            (content, probed)
                        }
                    }
                    _ => (None, true),
                };
                (Self::Resolved { kind, content }, content_probed)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Self::Broken, true),
            Err(_) => (Self::Unreachable, true),
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
