//! Copy and move original paths without merging or replacing existing targets.

use std::{
    collections::HashSet,
    io,
    path::{Component, Path, PathBuf},
};

use super::names::{self, NamedTarget};

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use rustix::{
    fs::{
        AtFlags, CWD, Dir, FileType, OFlags, RenameFlags, Stat, Timespec, Timestamps, UTIME_OMIT,
        futimens, utimensat,
    },
    io::Errno,
};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt, symlink},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Copy,
    Move,
}

/// How one planned transfer entry handles an existing target at its destination path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CollisionPolicy {
    /// Fail atomically with [`Error::Conflict`] if the destination already exists.
    #[default]
    Fail,
    /// Atomically replace an existing non-directory destination (file or symlink).
    /// Replacing an existing directory is still refused with [`Error::Conflict`].
    ReplaceNonDirectory,
}

/// One planned transfer entry with an explicit destination directory, target leaf name, and
/// collision policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedEntry {
    pub source: PathBuf,
    pub destination_dir: PathBuf,
    pub target_name: OsString,
    pub policy: CollisionPolicy,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub paths: Vec<PathBuf>,
    pub destination: PathBuf,
    pub mode: Mode,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("operation cancelled")]
    Cancelled,
    #[error("no files were selected for transfer")]
    EmptySelection,
    #[error("an absolute named source entry is required: {path}")]
    InvalidSource { path: PathBuf },
    #[error("invalid target name for {path}: {source}")]
    InvalidName {
        path: PathBuf,
        #[source]
        source: names::NameError,
    },
    #[error("two sources would be transferred to the same name: {name}")]
    DuplicateName { name: String },
    #[error("an absolute destination directory is required: {path}")]
    InvalidDestination { path: PathBuf },
    #[error("destination is not a directory: {path}")]
    NotDirectory { path: PathBuf },
    #[error("destination already exists; nothing was overwritten: {path}")]
    Conflict { path: PathBuf },
    #[error("cannot transfer a directory into itself or its descendant: {path}")]
    IntoSelf { path: PathBuf },
    #[error(
        "unsupported filesystem entry (only files, directories and links can be copied): {path}"
    )]
    UnsupportedEntry { path: PathBuf },
    #[error("source entry changed during transfer; its replacement was not removed: {path}")]
    SourceChanged { path: PathBuf },
    #[error("{operation} failed for {path}: {source}")]
    Filesystem {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "destination was committed at {destination}, but source removal failed for {path}: {source}"
    )]
    SourceRemoval {
        path: PathBuf,
        destination: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not clean staging {path}: {source}; preceding operation: {operation:?}")]
    StagingCleanup {
        path: PathBuf,
        #[source]
        source: io::Error,
        operation: Option<Box<Error>>,
    },
    #[error("atomic no-replace file transfers are currently supported only on Linux and FreeBSD")]
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    UnsupportedPlatform,
}

impl Error {
    fn filesystem(operation: &'static str, path: &Path, source: impl Into<io::Error>) -> Self {
        Self::Filesystem {
            operation,
            path: path.to_owned(),
            source: source.into(),
        }
    }
}

pub const DEFAULT_COPY_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct Batch {
    mode: Mode,
    entries: Vec<PlannedEntry>,
    cancel: tokio_util::sync::CancellationToken,
    buffer_size: usize,
}

impl Batch {
    /// Freeze unique paths. Filesystem inspection and all transfer work happen in `execute`.
    ///
    /// # Errors
    /// Rejects empty selections, relative destinations and sources without an absolute named leaf.
    pub fn new(request: Request) -> Result<Self, Error> {
        Self::named(request, Vec::new())
    }

    pub fn with_buffer_size(mut self, bytes: usize) -> Self {
        self.buffer_size = bytes.max(4096);
        self
    }

    /// Freeze the same request with explicit target leaves for some sources.
    ///
    /// Names outside `request.paths` are ignored, since they belong to a source
    /// that already succeeded. Duplicate target names are rejected here rather
    /// than turning into an implicit overwrite during execution.
    ///
    /// # Errors
    /// Reports the same request errors as [`Batch::new`], invalid leaf names and
    /// two sources that would share one target name.
    pub fn named(mut request: Request, targets: Vec<NamedTarget>) -> Result<Self, Error> {
        if !request.destination.is_absolute() {
            return Err(Error::InvalidDestination {
                path: request.destination,
            });
        }
        let mut seen = HashSet::new();
        for path in &request.paths {
            if !path.is_absolute()
                || !matches!(path.components().next_back(), Some(Component::Normal(_)))
            {
                return Err(Error::InvalidSource { path: path.clone() });
            }
        }
        request.paths.retain(|path| seen.insert(path.clone()));
        if request.paths.is_empty() {
            return Err(Error::EmptySelection);
        }
        let mut named = Vec::with_capacity(targets.len());
        let mut sources = HashSet::new();
        for target in targets {
            if !request.paths.contains(&target.source) || !sources.insert(target.source.clone()) {
                continue;
            }
            names::validate(&target.name).map_err(|source| Error::InvalidName {
                path: target.source.clone(),
                source,
            })?;
            if named
                .iter()
                .any(|existing: &NamedTarget| existing.name == target.name)
            {
                return Err(Error::DuplicateName {
                    name: target.name.to_string_lossy().escape_debug().to_string(),
                });
            }
            named.push(target);
        }
        let entries = request
            .paths
            .into_iter()
            .map(|source| {
                let target_name = names::target_name(&source, &named).to_owned();
                PlannedEntry {
                    source,
                    destination_dir: request.destination.clone(),
                    target_name,
                    policy: CollisionPolicy::Fail,
                }
            })
            .collect();
        Self::planned(request.mode, entries)
    }

    /// Freeze an explicit list of planned entries, each with its own destination directory, target
    /// leaf name, and collision policy.
    ///
    /// # Errors
    /// Rejects empty lists, relative paths, invalid leaf names, and two entries targeting the same
    /// `(destination_dir, target_name)`.
    pub fn planned(mode: Mode, entries: Vec<PlannedEntry>) -> Result<Self, Error> {
        if entries.is_empty() {
            return Err(Error::EmptySelection);
        }
        let mut planned_targets: HashSet<(PathBuf, OsString)> = HashSet::new();
        for entry in &entries {
            if !entry.destination_dir.is_absolute() {
                return Err(Error::InvalidDestination {
                    path: entry.destination_dir.clone(),
                });
            }
            if !entry.source.is_absolute()
                || !matches!(
                    entry.source.components().next_back(),
                    Some(Component::Normal(_))
                )
            {
                return Err(Error::InvalidSource {
                    path: entry.source.clone(),
                });
            }
            names::validate(&entry.target_name).map_err(|source| Error::InvalidName {
                path: entry.source.clone(),
                source,
            })?;
            if !planned_targets.insert((entry.destination_dir.clone(), entry.target_name.clone())) {
                return Err(Error::DuplicateName {
                    name: entry
                        .target_name
                        .to_string_lossy()
                        .escape_debug()
                        .to_string(),
                });
            }
        }
        Ok(Self {
            mode,
            entries,
            cancel: tokio_util::sync::CancellationToken::new(),
            buffer_size: DEFAULT_COPY_BUFFER_SIZE,
        })
    }

    /// Run on a background executor. Each item retains its own outcome and original paths.
    pub fn execute_cancellable(mut self, cancel: tokio_util::sync::CancellationToken) -> Report {
        self.cancel = cancel;
        self.execute()
    }

    pub fn execute(self) -> Report {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            self.execute_local()
        }
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            let Batch { mode, entries, .. } = self;
            let items = entries
                .into_iter()
                .map(|entry| {
                    let destination = entry.destination_dir.join(&entry.target_name);
                    Item {
                        source: entry.source,
                        destination,
                        policy: entry.policy,
                        replaced: false,
                        destination_committed: false,
                        committed_snapshot: None,
                        result: Err(Error::UnsupportedPlatform),
                    }
                })
                .collect();
            Report { mode, items }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn execute_local(self) -> Report {
        let Batch {
            mode,
            entries,
            cancel,
            buffer_size,
        } = self;
        let items = entries
            .into_iter()
            .map(|entry| {
                let destination = entry.destination_dir.join(&entry.target_name);
                let mut item = Item {
                    source: entry.source,
                    destination,
                    policy: entry.policy,
                    replaced: false,
                    destination_committed: false,
                    committed_snapshot: None,
                    result: Ok(()),
                };
                item.result = if cancel.is_cancelled() {
                    Err(Error::Cancelled)
                } else {
                    item.transfer(mode, buffer_size, &cancel)
                };
                if item.result.is_ok() {
                    if mode == Mode::Move {
                        gds::service::file_inspection::invalidate_thumbnail_cache(&item.source);
                    }
                    if item.replaced {
                        gds::service::file_inspection::invalidate_thumbnail_cache(
                            &item.destination,
                        );
                    }
                }
                item
            })
            .collect();
        Report { mode, items }
    }
}

/// Checks whether transferring directory `source` into `destination_dir` would place it into
/// itself or one of its descendants (resolving canonical parent paths).
///
/// # Errors
/// Returns [`Error::IntoSelf`] when `destination_dir` resolves inside `source`, or filesystem
/// inspection errors if paths cannot be resolved.
pub fn check_into_self(source: &Path, destination_dir: &Path) -> Result<(), Error> {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let parent = source.parent().ok_or_else(|| Error::InvalidSource {
            path: source.to_owned(),
        })?;
        let leaf = source.file_name().ok_or_else(|| Error::InvalidSource {
            path: source.to_owned(),
        })?;
        let canonical_source = parent
            .canonicalize()
            .map_err(|error| Error::filesystem("resolve source parent", parent, error))?
            .join(leaf);
        let metadata = fs::symlink_metadata(&canonical_source)
            .map_err(|error| Error::filesystem("inspect source", source, error))?;
        let canonical_dir = destination_dir
            .canonicalize()
            .map_err(|error| Error::filesystem("resolve destination", destination_dir, error))?;
        if metadata.is_dir() && canonical_dir.starts_with(&canonical_source) {
            return Err(Error::IntoSelf {
                path: destination_dir.to_owned(),
            });
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        let _ = (source, destination_dir);
        Err(Error::UnsupportedPlatform)
    }
}

/// Removes `path` via `unlinkat(..., REMOVEDIR)` only when it is still an empty real directory.
///
/// Returns `Ok(true)` if removed, `Ok(false)` if already gone or non-empty (preserving skipped or
/// concurrently added children), and `Err` on unexpected filesystem failures.
///
/// # Errors
/// Propagates permission or I/O errors other than `ENOENT`, `ENOTEMPTY`, `EEXIST`, or `ENOTDIR`.
pub fn remove_empty_dir_if_empty(path: &Path) -> Result<bool, Error> {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let parent_path = path.parent().ok_or_else(|| Error::InvalidSource {
            path: path.to_owned(),
        })?;
        let leaf = path.file_name().ok_or_else(|| Error::InvalidSource {
            path: path.to_owned(),
        })?;
        let parent = match File::open(parent_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(Error::filesystem("open directory parent", path, error)),
        };
        let stat = match rustix::fs::statat(&parent, leaf, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(Errno::NOENT) => return Ok(false),
            Err(error) => {
                return Err(Error::filesystem(
                    "inspect merged source directory",
                    path,
                    error,
                ));
            }
        };
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
            return Ok(false);
        }
        match rustix::fs::unlinkat(&parent, leaf, AtFlags::REMOVEDIR) {
            Ok(()) => Ok(true),
            Err(Errno::NOENT | Errno::NOTEMPTY | Errno::EXIST | Errno::NOTDIR) => Ok(false),
            Err(error) => Err(Error::filesystem(
                "remove empty merged directory",
                path,
                error,
            )),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        let _ = path;
        Err(Error::UnsupportedPlatform)
    }
}

#[derive(Debug)]
pub struct Item {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub policy: CollisionPolicy,
    /// True when an existing non-directory destination was replaced atomically.
    pub replaced: bool,
    /// Also true when a later source-removal or staging-cleanup error leaves a complete destination.
    pub destination_committed: bool,
    pub committed_snapshot: Option<crate::features::file_operations::history::Snapshot>,
    pub result: Result<(), Error>,
}

#[derive(Debug)]
pub struct Report {
    pub mode: Mode,
    pub items: Vec<Item>,
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
struct Prepared {
    source: PathBuf,
    destination: PathBuf,
    directory: PathBuf,
    metadata: fs::Metadata,
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl Prepared {
    /// # Errors
    /// Reports unavailable paths, unsupported entries and a resolved destination within the source.
    fn new(item: &Item) -> Result<Self, Error> {
        let parent = item
            .source
            .parent()
            .expect("validated absolute source leaf");
        let source = parent
            .canonicalize()
            .map_err(|error| Error::filesystem("resolve source parent", parent, error))?
            .join(item.source.file_name().expect("validated source leaf"));
        let metadata = fs::symlink_metadata(&source)
            .map_err(|error| Error::filesystem("inspect source", &item.source, error))?;
        if !metadata.is_file() && !metadata.is_dir() && !metadata.file_type().is_symlink() {
            return Err(Error::UnsupportedEntry {
                path: item.source.clone(),
            });
        }
        let directory = item
            .destination
            .parent()
            .expect("destination was joined to an absolute directory")
            .canonicalize()
            .map_err(|error| Error::filesystem("resolve destination", &item.destination, error))?;
        if !fs::metadata(&directory)
            .map_err(|error| Error::filesystem("inspect destination", &directory, error))?
            .is_dir()
        {
            return Err(Error::NotDirectory { path: directory });
        }
        if metadata.is_dir() && directory.starts_with(&source) {
            return Err(Error::IntoSelf {
                path: item.destination.clone(),
            });
        }
        let destination = directory.join(
            item.destination
                .file_name()
                .expect("validated destination leaf"),
        );
        Ok(Self {
            source,
            destination,
            directory,
            metadata,
        })
    }

    /// # Errors
    /// Returns the kernel's atomic rename failure. No replacing-rename fallback is used.
    fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), Errno> {
        rustix::fs::renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE)
    }

    /// Atomically replaces a non-directory `destination` with `source` via POSIX `renameat`.
    /// The kernel rejects replacing an existing directory with `EISDIR` / `ENOTEMPTY`.
    ///
    /// # Errors
    /// Returns the kernel rename failure.
    fn rename_replace(source: &Path, destination: &Path) -> Result<(), Errno> {
        rustix::fs::renameat(CWD, source, CWD, destination)
    }

    /// Validates destination according to `policy` before starting a copy or replacing move.
    /// Returns `Ok(true)` when an existing non-directory destination will be replaced.
    ///
    /// # Errors
    /// Reports [`Error::Conflict`] when `Fail` encounters an existing path or when
    /// `ReplaceNonDirectory` encounters a directory or self-overwrite.
    fn check_destination(&self, policy: CollisionPolicy) -> Result<bool, Error> {
        if self.source == self.destination {
            return Err(Error::Conflict {
                path: self.destination.clone(),
            });
        }
        match fs::symlink_metadata(&self.destination) {
            Ok(existing) => match policy {
                CollisionPolicy::Fail => Err(Error::Conflict {
                    path: self.destination.clone(),
                }),
                CollisionPolicy::ReplaceNonDirectory => {
                    if self.metadata.is_dir() || existing.is_dir() {
                        Err(Error::Conflict {
                            path: self.destination.clone(),
                        })
                    } else {
                        Ok(true)
                    }
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::filesystem(
                "inspect target",
                &self.destination,
                error,
            )),
        }
    }

    fn rename_error(&self, error: Errno) -> Error {
        if matches!(error, Errno::EXIST | Errno::NOTEMPTY | Errno::ISDIR) {
            Error::Conflict {
                path: self.destination.clone(),
            }
        } else {
            Error::filesystem("commit without replacing", &self.destination, error)
        }
    }

    /// # Errors
    /// Reports a changed source or removal failure after the complete destination was committed.
    fn remove_source(&self, copied: &Copied) -> Result<(), Error> {
        let parent = File::open(self.source.parent().expect("prepared source has parent"))
            .map_err(|error| Error::filesystem("open source parent", &self.source, error))?;
        copied
            .remove_at(
                &parent,
                self.source.file_name().expect("prepared source has leaf"),
                &self.source,
            )
            .map_err(|error| match error {
                Error::Filesystem { source, path, .. } => Error::SourceRemoval {
                    path,
                    destination: self.destination.clone(),
                    source,
                },
                error => error,
            })
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl Item {
    /// # Errors
    /// Retains the source on copy/commit failure; a post-commit failure is accompanied by `destination_committed`.
    fn transfer(
        &mut self,
        mode: Mode,
        buffer_size: usize,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(), Error> {
        let prepared = Prepared::new(self)?;
        let policy = self.policy;
        let will_replace = prepared.check_destination(policy)?;
        if mode == Mode::Move {
            let rename_result = match policy {
                CollisionPolicy::Fail => {
                    Prepared::rename_no_replace(&prepared.source, &prepared.destination)
                }
                CollisionPolicy::ReplaceNonDirectory => {
                    Prepared::rename_replace(&prepared.source, &prepared.destination)
                }
            };
            match rename_result {
                Ok(()) => {
                    self.replaced = will_replace || policy == CollisionPolicy::ReplaceNonDirectory;
                    self.destination_committed = true;
                    self.committed_snapshot =
                        crate::features::file_operations::history::Snapshot::capture(
                            &prepared.destination,
                        )
                        .ok();
                    return Ok(());
                }
                Err(Errno::XDEV) => {}
                Err(error) => return Err(prepared.rename_error(error)),
            }
        }
        let stage = Staging::new(&prepared.directory)?;
        let node = Node {
            source: prepared.source.clone(),
            destination: stage.payload(),
            cancel: cancel.clone(),
            buffer_size,
        };
        let mut copied = None;
        let result = node.copy_root().and_then(|manifest| {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if manifest.metadata.st_dev != prepared.metadata.dev()
                || manifest.metadata.st_ino != prepared.metadata.ino()
            {
                return Err(Error::SourceChanged {
                    path: prepared.source.clone(),
                });
            }
            let commit_result = match policy {
                CollisionPolicy::Fail => {
                    Prepared::rename_no_replace(&node.destination, &prepared.destination)
                }
                CollisionPolicy::ReplaceNonDirectory => {
                    Prepared::rename_replace(&node.destination, &prepared.destination)
                }
            };
            commit_result.map_err(|error| prepared.rename_error(error))?;
            self.replaced = will_replace || policy == CollisionPolicy::ReplaceNonDirectory;
            self.destination_committed = true;
            self.committed_snapshot =
                crate::features::file_operations::history::Snapshot::capture(&prepared.destination)
                    .ok();
            copied = Some(manifest);
            Ok(())
        });
        stage.finish(result)?;
        if mode == Mode::Move {
            prepared.remove_source(copied.as_ref().expect("successful copy has manifest"))?;
        }
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
struct Node {
    cancel: tokio_util::sync::CancellationToken,
    source: PathBuf,
    destination: PathBuf,
    buffer_size: usize,
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl Node {
    /// # Errors
    /// Reports source-parent access or recursive-copy errors.
    fn copy_root(&self) -> Result<Copied, Error> {
        let parent = File::open(self.source.parent().expect("prepared source has parent"))
            .map_err(|error| Error::filesystem("open source parent", &self.source, error))?;
        self.copy_at(
            &parent,
            self.source.file_name().expect("prepared source has leaf"),
        )
    }

    /// # Errors
    /// Refuses special entries and propagates reading, writing and permission errors.
    fn copy_at(&self, parent: &File, name: &OsStr) -> Result<Copied, Error> {
        if self.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let metadata = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| Error::filesystem("inspect source", &self.source, error))?;
        let children = match FileType::from_raw_mode(metadata.st_mode) {
            FileType::RegularFile => {
                self.copy_file(parent, name, &metadata)?;
                Vec::new()
            }
            FileType::Directory => self.copy_directory(parent, name, &metadata)?,
            FileType::Symlink => {
                let target = rustix::fs::readlinkat(parent, name, Vec::new())
                    .map_err(|error| Error::filesystem("read link", &self.source, error))?;
                symlink(OsStr::from_bytes(target.to_bytes()), &self.destination)
                    .map_err(|error| Error::filesystem("create link", &self.destination, error))?;
                Self::preserve_path_timestamps(&self.destination, &metadata)?;
                Vec::new()
            }
            _ => {
                return Err(Error::UnsupportedEntry {
                    path: self.source.clone(),
                });
            }
        };
        let after = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| Error::filesystem("recheck copied source", &self.source, error))?;
        Copied::check(&metadata, &after, &self.source)?;
        Ok(Copied { metadata, children })
    }

    /// # Errors
    /// Reports a changed entry type or file read/write/permission failures.
    fn copy_file(&self, parent: &File, name: &OsStr, expected: &Stat) -> Result<(), Error> {
        // NOFOLLOW prevents a changed leaf from opening a link; NONBLOCK prevents a changed FIFO from hanging.
        let descriptor = rustix::fs::openat(
            parent,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| Error::filesystem("open file", &self.source, error))?;
        let mut source = File::from(descriptor);
        let opened = rustix::fs::fstat(&source)
            .map_err(|error| Error::filesystem("inspect open file", &self.source, error))?;
        Copied::check(expected, &opened, &self.source)?;
        if FileType::from_raw_mode(opened.st_mode) != FileType::RegularFile {
            return Err(Error::UnsupportedEntry {
                path: self.source.clone(),
            });
        }
        let permissions = Self::permissions_from_stat(&opened);
        let mut destination = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.destination)
            .map_err(|error| Error::filesystem("create staged file", &self.destination, error))?;
        use std::io::{Read, Write};
        let mut buffer = vec![0; self.buffer_size.max(4096)];
        loop {
            if self.cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let count = source
                .read(&mut buffer)
                .map_err(|error| Error::filesystem("read file", &self.source, error))?;
            if count == 0 {
                break;
            }
            destination
                .write_all(&buffer[..count])
                .map_err(|error| Error::filesystem("copy file", &self.destination, error))?;
        }
        gds::service::file_tags::copy_from(&source, &self.destination)
            .map_err(|error| Error::filesystem("preserve file tags", &self.destination, error))?;
        let after = rustix::fs::fstat(&source)
            .map_err(|error| Error::filesystem("recheck open file", &self.source, error))?;
        Copied::check(expected, &after, &self.source)?;
        Self::preserve_file_timestamps(&destination, &opened, &self.destination)?;
        destination
            .set_permissions(permissions)
            .map_err(|error| Error::filesystem("set file permissions", &self.destination, error))
    }

    /// # Errors
    /// Reports inaccessible entries or incomplete copies; source links are never traversed.
    fn copy_directory(
        &self,
        parent: &File,
        name: &OsStr,
        expected: &Stat,
    ) -> Result<Vec<CopiedChild>, Error> {
        let descriptor = rustix::fs::openat(
            parent,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| Error::filesystem("open directory", &self.source, error))?;
        let source = File::from(descriptor);
        let opened = rustix::fs::fstat(&source)
            .map_err(|error| Error::filesystem("inspect open directory", &self.source, error))?;
        Copied::check(expected, &opened, &self.source)?;
        let permissions = Self::permissions_from_stat(&opened);
        fs::create_dir(&self.destination).map_err(|error| {
            Error::filesystem("create staged directory", &self.destination, error)
        })?;
        let entries = Dir::read_from(&source)
            .map_err(|error| Error::filesystem("read directory", &self.source, error))?;
        let mut children = Vec::new();
        for entry in entries {
            let entry = entry
                .map_err(|error| Error::filesystem("read directory entry", &self.source, error))?;
            let name = OsStr::from_bytes(entry.file_name().to_bytes());
            if name == "." || name == ".." {
                continue;
            }
            let copied = Self {
                cancel: self.cancel.clone(),
                source: self.source.join(name),
                destination: self.destination.join(name),
                buffer_size: self.buffer_size,
            }
            .copy_at(&source, name)?;
            children.push(CopiedChild {
                name: name.to_owned(),
                copied,
            });
        }
        // Apply timestamps and readonly source modes only after every child has been written.
        gds::service::file_tags::copy_from(&source, &self.destination).map_err(|error| {
            Error::filesystem("preserve directory tags", &self.destination, error)
        })?;
        Self::preserve_path_timestamps(&self.destination, &opened)?;
        fs::set_permissions(&self.destination, permissions).map_err(|error| {
            Error::filesystem("set directory permissions", &self.destination, error)
        })?;
        let after = rustix::fs::fstat(&source)
            .map_err(|error| Error::filesystem("recheck open directory", &self.source, error))?;
        Copied::check(expected, &after, &self.source)?;
        Ok(children)
    }

    #[allow(clippy::useless_conversion)]
    fn permissions_from_stat(source: &Stat) -> fs::Permissions {
        fs::Permissions::from_mode(u32::from(source.st_mode))
    }

    fn source_timestamps(source: &Stat) -> Timestamps {
        Timestamps {
            last_access: Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_OMIT,
            },
            last_modification: Timespec {
                tv_sec: source.st_mtime as _,
                tv_nsec: source.st_mtime_nsec as _,
            },
        }
    }

    fn is_unsupported_timestamp_error(error: Errno) -> bool {
        matches!(
            error,
            Errno::PERM | Errno::ACCESS | Errno::OPNOTSUPP | Errno::NOSYS
        )
    }

    /// # Errors
    /// Propagates unexpected kernel timestamp failures while ignoring mount-level denials.
    fn preserve_file_timestamps(file: &File, source: &Stat, path: &Path) -> Result<(), Error> {
        match futimens(file, &Self::source_timestamps(source)) {
            Ok(()) => Ok(()),
            Err(error) if Self::is_unsupported_timestamp_error(error) => Ok(()),
            Err(error) => Err(Error::filesystem("preserve timestamps", path, error)),
        }
    }

    /// # Errors
    /// Propagates unexpected kernel timestamp failures while ignoring mount-level denials.
    fn preserve_path_timestamps(path: &Path, source: &Stat) -> Result<(), Error> {
        match utimensat(
            CWD,
            path,
            &Self::source_timestamps(source),
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(()) => Ok(()),
            Err(error) if Self::is_unsupported_timestamp_error(error) => Ok(()),
            Err(error) => Err(Error::filesystem("preserve timestamps", path, error)),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
struct CopiedChild {
    name: OsString,
    copied: Copied,
}

/// Only entries actually copied may be removed. Never enumerate the live tree during cleanup.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
struct Copied {
    metadata: Stat,
    children: Vec<CopiedChild>,
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl Copied {
    fn same_identity(expected: &Stat, current: &Stat) -> bool {
        expected.st_dev == current.st_dev
            && expected.st_ino == current.st_ino
            && expected.st_mode == current.st_mode
    }

    /// # Errors
    /// Reports [`Error::SourceChanged`] when file identity, size or timestamps differ.
    fn check(expected: &Stat, current: &Stat, path: &Path) -> Result<(), Error> {
        if Self::same_identity(expected, current)
            && expected.st_size == current.st_size
            && expected.st_mtime == current.st_mtime
            && expected.st_mtime_nsec == current.st_mtime_nsec
            && expected.st_ctime == current.st_ctime
            && expected.st_ctime_nsec == current.st_ctime_nsec
        {
            Ok(())
        } else {
            Err(Error::SourceChanged {
                path: path.to_owned(),
            })
        }
    }

    /// # Errors
    /// Retains changed entries and nonempty directories; reports partial cleanup.
    fn remove_at(&self, parent: &File, name: &OsStr, path: &Path) -> Result<(), Error> {
        let current = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| Error::filesystem("recheck source", path, error))?;
        Self::check(&self.metadata, &current, path)?;
        let directory = FileType::from_raw_mode(current.st_mode) == FileType::Directory;
        if directory {
            let descriptor = rustix::fs::openat(
                parent,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(|error| Error::filesystem("open source directory for cleanup", path, error))?;
            let source = File::from(descriptor);
            let opened = rustix::fs::fstat(&source)
                .map_err(|error| Error::filesystem("inspect cleanup directory", path, error))?;
            Self::check(&self.metadata, &opened, path)?;
            for child in &self.children {
                child
                    .copied
                    .remove_at(&source, &child.name, &path.join(&child.name))?;
            }
            // Removing our children changes directory timestamps. Recheck identity, not timestamps.
            let current = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|error| Error::filesystem("recheck cleanup directory", path, error))?;
            if !Self::same_identity(&self.metadata, &current) {
                return Err(Error::SourceChanged {
                    path: path.to_owned(),
                });
            }
        }
        // stat→unlink is not atomic with external writers; no cross-filesystem snapshot is promised.
        // rmdir, unlike recursive deletion, cannot delete an uncopied late child.
        rustix::fs::unlinkat(
            parent,
            name,
            if directory {
                AtFlags::REMOVEDIR
            } else {
                AtFlags::empty()
            },
        )
        .map_err(|error| Error::filesystem("remove copied source", path, error))
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
struct Staging {
    directory: tempfile::TempDir,
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl Staging {
    /// # Errors
    /// Reports an unwritable or unavailable destination directory.
    fn new(destination: &Path) -> Result<Self, Error> {
        tempfile::Builder::new()
            .prefix(".gds-transfer-")
            .tempdir_in(destination)
            .map(|directory| Self { directory })
            .map_err(|error| Error::filesystem("create staging directory", destination, error))
    }
    fn payload(&self) -> PathBuf {
        self.directory.path().join("entry")
    }

    /// # Errors
    /// Propagates the operation error and explicitly reports cleanup failures, retaining both errors when needed.
    fn finish(self, operation: Result<(), Error>) -> Result<(), Error> {
        let path = self.directory.path().to_owned();
        let cleanup = if operation.is_err() {
            Self::make_removable(&path).and_then(|()| self.directory.close())
        } else {
            self.directory.close()
        };
        match cleanup {
            Ok(()) => operation,
            Err(source) => Err(Error::StagingCleanup {
                path,
                source,
                operation: operation.err().map(Box::new),
            }),
        }
    }

    /// # Errors
    /// Reports failures to make this private staging tree removable; never follows copied links.
    fn make_removable(path: &Path) -> io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.is_dir() {
            fs::set_permissions(
                path,
                fs::Permissions::from_mode(metadata.permissions().mode() | 0o700),
            )?;
            for entry in fs::read_dir(path)? {
                Self::make_removable(&entry?.path())?;
            }
        }
        Ok(())
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "freebsd")))]
mod tests;
