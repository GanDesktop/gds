//! Copy and move original paths without merging or replacing existing targets.

use std::{
    collections::HashSet,
    io,
    path::{Component, Path, PathBuf},
};

use super::names::{self, NamedTarget};

#[cfg(target_os = "linux")]
use rustix::{
    fs::{AtFlags, CWD, Dir, FileType, OFlags, RenameFlags, Stat},
    io::Errno,
};
#[cfg(target_os = "linux")]
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

#[derive(Clone, Debug)]
pub struct Request {
    pub paths: Vec<PathBuf>,
    pub destination: PathBuf,
    pub mode: Mode,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
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
    #[error("atomic no-replace file transfers are currently supported only on Linux")]
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

#[derive(Clone, Debug)]
pub struct Batch {
    request: Request,
    /// Explicit target leaves; sources absent here keep their original name.
    names: Vec<NamedTarget>,
}

impl Batch {
    /// Freeze unique paths. Filesystem inspection and all transfer work happen in `execute`.
    ///
    /// # Errors
    /// Rejects empty selections, relative destinations and sources without an absolute named leaf.
    pub fn new(mut request: Request) -> Result<Self, Error> {
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
        Ok(Self {
            request,
            names: Vec::new(),
        })
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
    pub fn named(request: Request, targets: Vec<NamedTarget>) -> Result<Self, Error> {
        let mut batch = Self::new(request)?;
        let mut named = Vec::with_capacity(targets.len());
        let mut sources = HashSet::new();
        for target in targets {
            if !batch.request.paths.contains(&target.source)
                || !sources.insert(target.source.clone())
            {
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
        batch.names = named;
        Ok(batch)
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.request.paths
    }
    pub fn destination(&self) -> &Path {
        &self.request.destination
    }
    pub fn mode(&self) -> Mode {
        self.request.mode
    }

    /// Run on a background executor. Each item retains its own outcome and original paths.
    pub fn execute(self) -> Report {
        #[cfg(target_os = "linux")]
        {
            self.execute_with_rename(Prepared::rename_no_replace)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let Batch { request, names } = self;
            let mode = request.mode;
            let items = request
                .paths
                .into_iter()
                .map(|source| {
                    let destination = request
                        .destination
                        .join(names::target_name(&source, &names));
                    Item {
                        source,
                        destination,
                        destination_committed: false,
                        result: Err(Error::UnsupportedPlatform),
                    }
                })
                .collect();
            Report { mode, items }
        }
    }

    #[cfg(target_os = "linux")]
    fn execute_with_rename(
        self,
        mut initial_rename: impl FnMut(&Path, &Path) -> Result<(), Errno>,
    ) -> Report {
        let Batch { request, names } = self;
        let mode = request.mode;
        let items = request
            .paths
            .into_iter()
            .map(|source| {
                let destination = request
                    .destination
                    .join(names::target_name(&source, &names));
                let mut item = Item {
                    source,
                    destination,
                    destination_committed: false,
                    result: Ok(()),
                };
                item.result = item.transfer(mode, &mut initial_rename);
                item
            })
            .collect();
        Report { mode, items }
    }
}

#[derive(Debug)]
pub struct Item {
    pub source: PathBuf,
    pub destination: PathBuf,
    /// Also true when a later source-removal or staging-cleanup error leaves a complete destination.
    pub destination_committed: bool,
    pub result: Result<(), Error>,
}

#[derive(Debug)]
pub struct Report {
    pub mode: Mode,
    pub items: Vec<Item>,
}

impl Report {
    pub fn succeeded_sources(&self) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter(|item| item.result.is_ok())
            .map(|item| item.source.clone())
            .collect()
    }
    pub fn committed_destinations(&self) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter(|item| item.destination_committed)
            .map(|item| item.destination.clone())
            .collect()
    }
}

#[cfg(target_os = "linux")]
struct Prepared {
    source: PathBuf,
    destination: PathBuf,
    directory: PathBuf,
    metadata: fs::Metadata,
}

#[cfg(target_os = "linux")]
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

    /// # Errors
    /// Reports a conflict or an error inspecting the target. Commit checks again atomically.
    fn check_destination(&self) -> Result<(), Error> {
        match fs::symlink_metadata(&self.destination) {
            Ok(_) => Err(Error::Conflict {
                path: self.destination.clone(),
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::filesystem(
                "inspect target",
                &self.destination,
                error,
            )),
        }
    }

    fn rename_error(&self, error: Errno) -> Error {
        if matches!(error, Errno::EXIST | Errno::NOTEMPTY) {
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

#[cfg(target_os = "linux")]
impl Item {
    /// # Errors
    /// Retains the source on copy/commit failure; a post-commit failure is accompanied by `destination_committed`.
    fn transfer(
        &mut self,
        mode: Mode,
        initial_rename: &mut impl FnMut(&Path, &Path) -> Result<(), Errno>,
    ) -> Result<(), Error> {
        let prepared = Prepared::new(self)?;
        if mode == Mode::Move {
            match initial_rename(&prepared.source, &prepared.destination) {
                Ok(()) => {
                    self.destination_committed = true;
                    return Ok(());
                }
                Err(Errno::XDEV) => {}
                Err(error) => return Err(prepared.rename_error(error)),
            }
        }
        prepared.check_destination()?;
        let stage = Staging::new(&prepared.directory)?;
        let node = Node {
            source: prepared.source.clone(),
            destination: stage.payload(),
        };
        let mut copied = None;
        let result = node.copy_root().and_then(|manifest| {
            if manifest.metadata.st_dev != prepared.metadata.dev()
                || manifest.metadata.st_ino != prepared.metadata.ino()
            {
                return Err(Error::SourceChanged {
                    path: prepared.source.clone(),
                });
            }
            Prepared::rename_no_replace(&node.destination, &prepared.destination)
                .map_err(|error| prepared.rename_error(error))?;
            self.destination_committed = true;
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

#[cfg(target_os = "linux")]
struct Node {
    source: PathBuf,
    destination: PathBuf,
}

#[cfg(target_os = "linux")]
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
        let metadata = source
            .metadata()
            .map_err(|error| Error::filesystem("inspect open file", &self.source, error))?;
        if !metadata.is_file() {
            return Err(Error::UnsupportedEntry {
                path: self.source.clone(),
            });
        }
        let mut destination = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.destination)
            .map_err(|error| Error::filesystem("create staged file", &self.destination, error))?;
        io::copy(&mut source, &mut destination)
            .map_err(|error| Error::filesystem("copy file", &self.source, error))?;
        let after = rustix::fs::fstat(&source)
            .map_err(|error| Error::filesystem("recheck open file", &self.source, error))?;
        Copied::check(expected, &after, &self.source)?;
        destination
            .set_permissions(metadata.permissions())
            .map_err(|error| Error::filesystem("set file permissions", &self.destination, error))
    }

    /// # Errors
    /// Reports inaccessible entries or incomplete copies; source links are never traversed.
    fn copy_directory(
        &self,
        parent: &File,
        name: &OsStr,
        expected: &Stat,
    ) -> Result<Vec<(OsString, Copied)>, Error> {
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
        let permissions = source
            .metadata()
            .map_err(|error| Error::filesystem("inspect open directory", &self.source, error))?
            .permissions();
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
                source: self.source.join(name),
                destination: self.destination.join(name),
            }
            .copy_at(&source, name)?;
            children.push((name.to_owned(), copied));
        }
        // Apply readonly source modes only after every child has been written.
        fs::set_permissions(&self.destination, permissions).map_err(|error| {
            Error::filesystem("set directory permissions", &self.destination, error)
        })?;
        let after = rustix::fs::fstat(&source)
            .map_err(|error| Error::filesystem("recheck open directory", &self.source, error))?;
        Copied::check(expected, &after, &self.source)?;
        Ok(children)
    }
}

/// Only entries actually copied may be removed. Never enumerate the live tree during cleanup.
#[cfg(target_os = "linux")]
struct Copied {
    metadata: Stat,
    children: Vec<(OsString, Copied)>,
}

#[cfg(target_os = "linux")]
impl Copied {
    fn same_identity(expected: &Stat, current: &Stat) -> bool {
        expected.st_dev == current.st_dev
            && expected.st_ino == current.st_ino
            && expected.st_mode == current.st_mode
    }

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
            for (child, copied) in &self.children {
                copied.remove_at(&source, child, &path.join(child))?;
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

#[cfg(target_os = "linux")]
struct Staging {
    directory: tempfile::TempDir,
}

#[cfg(target_os = "linux")]
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
        let cleanup = Self::make_removable(&path).and_then(|()| self.directory.close());
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

#[cfg(all(test, target_os = "linux"))]
mod tests;
