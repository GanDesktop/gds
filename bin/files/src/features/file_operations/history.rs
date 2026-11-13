//! Session-local file-operation history. Each completed effect supplies its inverse.
//!
//! Inspired by Nautilus' [undo manager](https://gitlab.gnome.org/GNOME/nautilus/-/blob/main/src/nautilus-file-undo-manager.c):
//! replay does not register a new user operation.
//! Batches retain only completed effects, including after partial failures.

use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use super::{names::NamedTarget, recycle, transfer};

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

    /// # Errors
    /// Maps missing entries to [`Error::TargetChanged`] and other failures to [`Error::TargetInspection`].
    fn inspect(path: &Path) -> Result<Self, Error> {
        Self::capture(path).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                Error::TargetChanged {
                    path: path.to_owned(),
                }
            } else {
                Error::TargetInspection {
                    path: path.to_owned(),
                    source,
                }
            }
        })
    }

    fn same_identity(&self, current: &Self) -> bool {
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

    /// Verify that `path` still refers to the same filesystem object (allowing in-place edits).
    ///
    /// # Errors
    /// Reports missing/replaced entries as [`Error::TargetChanged`] and I/O failures.
    fn verify_identity(&self, path: &Path) -> Result<(), Error> {
        let current = Self::inspect(path)?;
        if self.same_identity(&current) {
            Ok(())
        } else {
            Err(Error::TargetChanged {
                path: path.to_owned(),
            })
        }
    }

    /// Verify that `path` still refers to the same unmodified entry.
    ///
    /// # Errors
    /// Reports missing, replaced or modified entries as [`Error::TargetChanged`] and I/O failures.
    fn verify_unmodified(&self, path: &Path) -> Result<(), Error> {
        let current = Self::inspect(path)?;
        if *self == current {
            Ok(())
        } else {
            Err(Error::TargetChanged {
                path: path.to_owned(),
            })
        }
    }

    /// Verify that `path` still refers to the exact created directory and contains no entries.
    ///
    /// # Errors
    /// Reports replaced directories as [`Error::TargetChanged`], occupied directories as
    /// [`Error::DirectoryNotEmpty`], and I/O failures.
    fn verify_empty_directory(&self, path: &Path) -> Result<(), Error> {
        #[cfg(target_os = "linux")]
        {
            use rustix::{
                fs::{CWD, Dir, Mode, OFlags, fstat, openat},
                io::Errno,
            };
            use std::os::unix::ffi::OsStrExt as _;

            let descriptor = openat(
                CWD,
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| match error {
                Errno::NOENT | Errno::NOTDIR | Errno::LOOP => Error::TargetChanged {
                    path: path.to_owned(),
                },
                other => Error::TargetInspection {
                    path: path.to_owned(),
                    source: other.into(),
                },
            })?;
            let directory = fs::File::from(descriptor);
            let stat = fstat(&directory).map_err(|error| Error::TargetInspection {
                path: path.to_owned(),
                source: error.into(),
            })?;
            if self.kind != TargetKind::Directory
                || self.device != stat.st_dev
                || self.inode != stat.st_ino
            {
                return Err(Error::TargetChanged {
                    path: path.to_owned(),
                });
            }
            let entries = Dir::read_from(&directory).map_err(|error| Error::TargetInspection {
                path: path.to_owned(),
                source: error.into(),
            })?;
            for entry in entries {
                let entry = entry.map_err(|error| Error::TargetInspection {
                    path: path.to_owned(),
                    source: error.into(),
                })?;
                let name = std::ffi::OsStr::from_bytes(entry.file_name().to_bytes());
                if name != "." && name != ".." {
                    return Err(Error::DirectoryNotEmpty {
                        path: path.to_owned(),
                    });
                }
            }
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            if self.kind != TargetKind::Directory {
                return Err(Error::TargetChanged {
                    path: path.to_owned(),
                });
            }
            self.verify_identity(path)?;
            let mut entries = fs::read_dir(path).map_err(|source| Error::TargetInspection {
                path: path.to_owned(),
                source,
            })?;
            if entries
                .next()
                .transpose()
                .map_err(|source| Error::TargetInspection {
                    path: path.to_owned(),
                    source,
                })?
                .is_some()
            {
                return Err(Error::DirectoryNotEmpty {
                    path: path.to_owned(),
                });
            }
            Ok(())
        }
    }
}

pub fn transfer_steps(report: &transfer::Report) -> Vec<Step> {
    report
        .items
        .iter()
        .filter(|item| {
            item.result.is_ok()
                || (report.mode == transfer::Mode::Copy && item.destination_committed)
        })
        .filter_map(|item| {
            let expected = item.committed_snapshot?;
            Some(match report.mode {
                transfer::Mode::Copy => Step::trash(item.destination.clone(), expected),
                transfer::Mode::Move => Step::Move {
                    source: item.destination.clone(),
                    destination: item.source.clone(),
                    expected,
                },
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Copy,
    Move,
    Rename,
    NewFolder,
    Trash,
    Restore,
}

impl Kind {
    pub fn label_id(self) -> &'static str {
        match self {
            Self::Copy => "history-copy",
            Self::Move => "history-move",
            Self::Rename => "history-rename",
            Self::NewFolder => "history-new-folder",
            Self::Trash => "history-trash",
            Self::Restore => "history-restore",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrashPolicy {
    Unmodified,
    EmptyDirectory,
}

#[derive(Clone, Debug)]
pub enum Step {
    Move {
        source: PathBuf,
        destination: PathBuf,
        expected: Snapshot,
    },
    Trash {
        path: PathBuf,
        previous: Option<OsString>,
        expected: Snapshot,
        policy: TrashPolicy,
    },
    Restore(recycle::Entry),
}

impl Step {
    pub fn trash(path: PathBuf, expected: Snapshot) -> Self {
        Self::Trash {
            path,
            previous: None,
            expected,
            policy: TrashPolicy::Unmodified,
        }
    }

    pub fn trash_empty_directory(path: PathBuf, expected: Snapshot) -> Self {
        Self::Trash {
            path,
            previous: None,
            expected,
            policy: TrashPolicy::EmptyDirectory,
        }
    }

    pub fn undo_restore(entry: &recycle::Entry, expected: Snapshot) -> Self {
        Self::Trash {
            path: entry.original_path.clone(),
            previous: Some(entry.id.clone()),
            expected,
            policy: TrashPolicy::Unmodified,
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        match self {
            Self::Move {
                source,
                destination,
                ..
            } => vec![source.clone(), destination.clone()],
            Self::Trash { path, .. } => vec![path.clone()],
            Self::Restore(entry) => vec![entry.original_path.clone()],
        }
    }

    /// # Errors
    /// Rejects targets whose identity, content stamp or directory emptiness changed.
    fn verify_trash(&self) -> Result<(), Error> {
        let Self::Trash {
            path,
            expected,
            policy,
            ..
        } = self
        else {
            return Ok(());
        };
        match policy {
            TrashPolicy::Unmodified => expected.verify_unmodified(path),
            TrashPolicy::EmptyDirectory => expected.verify_empty_directory(path),
        }
    }

    /// # Errors
    /// Propagates cancellation, target verification, move, trash or restore failures.
    fn apply(
        &self,
        kind: Kind,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<Self, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        match self {
            Self::Move {
                source,
                destination,
                expected,
            } => {
                expected.verify_identity(source)?;
                let parent = destination
                    .parent()
                    .ok_or_else(|| Error::InvalidPath(destination.clone()))?;
                let name = destination
                    .file_name()
                    .ok_or_else(|| Error::InvalidPath(destination.clone()))?;
                let batch = transfer::Batch::named(
                    transfer::Request {
                        paths: vec![source.clone()],
                        destination: parent.to_owned(),
                        mode: transfer::Mode::Move,
                    },
                    vec![NamedTarget::new(source.clone(), name.to_owned())],
                )?;
                let item = batch
                    .execute_cancellable(cancel.clone())
                    .items
                    .pop()
                    .expect("one move result");
                item.result?;
                let next = match item.committed_snapshot {
                    Some(snapshot) => snapshot,
                    None => Snapshot::inspect(destination)?,
                };
                Ok(Self::Move {
                    source: destination.clone(),
                    destination: source.clone(),
                    expected: next,
                })
            }
            Self::Trash { path, .. } => {
                self.verify_trash()?;
                Ok(Self::Restore(
                    recycle::trash_cancellable(path, cancel)?.entry?,
                ))
            }
            Self::Restore(entry) => {
                let outcome = recycle::perform_cancellable(
                    recycle::Action::Restore,
                    vec![entry.clone()],
                    cancel,
                )
                .pop()
                .expect("one restore result");
                outcome.result?;
                let expected = match outcome.restored {
                    Some(snapshot) => snapshot,
                    None => Snapshot::inspect(&entry.original_path)?,
                };
                let policy = if kind == Kind::NewFolder {
                    TrashPolicy::EmptyDirectory
                } else {
                    TrashPolicy::Unmodified
                };
                Ok(Self::Trash {
                    path: entry.original_path.clone(),
                    previous: Some(entry.id.clone()),
                    expected,
                    policy,
                })
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("operation cancelled")]
    Cancelled,
    #[error("invalid history target: {0}")]
    InvalidPath(PathBuf),
    #[error("history target changed or disappeared since the operation completed: {path}")]
    TargetChanged { path: PathBuf },
    #[error("cannot undo folder creation because the folder is no longer empty: {path}")]
    DirectoryNotEmpty { path: PathBuf },
    #[error("cannot inspect history target {path}: {source}")]
    TargetInspection {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Transfer(#[from] transfer::Error),
    #[error(transparent)]
    Trash(#[from] recycle::Error),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: Kind,
    pub steps: Vec<Step>,
}

struct PendingTrash {
    step: Step,
    previous: Option<OsString>,
    original: PathBuf,
}

impl Entry {
    pub fn new(kind: Kind, steps: Vec<Step>) -> Self {
        Self { kind, steps }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.steps.iter().flat_map(Step::paths).collect()
    }

    pub fn execute_cancellable(self, cancel: &tokio_util::sync::CancellationToken) -> Report {
        let mut report = Report {
            kind: self.kind,
            changed: self.paths(),
            inverse: Vec::new(),
            remaining: Vec::new(),
            errors: Vec::new(),
            replacements: Vec::new(),
        };
        let mut tracker = recycle::Tracker::new();
        let mut pending_trash: Vec<PendingTrash> = Vec::new();
        for step in self.steps.into_iter().rev() {
            if cancel.is_cancelled() {
                if report.remaining.is_empty() {
                    report.errors.push(Error::Cancelled.to_string());
                }
                report.remaining.push(step);
                continue;
            }
            if let Step::Trash { path, previous, .. } = &step {
                let deleted = step
                    .verify_trash()
                    .and_then(|()| tracker.delete(path, cancel).map_err(Error::from));
                match deleted {
                    Ok(original) => pending_trash.push(PendingTrash {
                        previous: previous.clone(),
                        step,
                        original,
                    }),
                    Err(error) => {
                        report.errors.push(error.to_string());
                        report.remaining.push(step);
                    }
                }
                continue;
            }
            match step.apply(self.kind, cancel) {
                Ok(inverse) => {
                    report.inverse.push(inverse);
                }
                Err(error) => {
                    report.errors.push(error.to_string());
                    report.remaining.push(step);
                }
            }
        }
        let originals = pending_trash
            .iter()
            .map(|item| item.original.clone())
            .collect();
        for (pending, receipt) in pending_trash.into_iter().zip(tracker.finish(originals)) {
            match receipt {
                Ok(current) => {
                    if let Some(previous) = pending.previous {
                        report.replacements.push(Replacement {
                            previous,
                            current: current.clone(),
                        });
                    }
                    report.inverse.push(Step::Restore(current));
                }
                Err(error) => {
                    report.errors.push(error.to_string());
                    report.remaining.push(pending.step);
                }
            }
        }
        report.remaining.reverse();
        report
    }
}

struct Replacement {
    previous: OsString,
    current: recycle::Entry,
}

pub struct Report {
    pub kind: Kind,
    pub changed: Vec<PathBuf>,
    pub inverse: Vec<Step>,
    pub remaining: Vec<Step>,
    pub errors: Vec<String>,
    replacements: Vec<Replacement>,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl History {
    pub fn record(&mut self, entry: Entry) {
        if !entry.steps.is_empty() {
            self.redo.clear();
            self.undo.push(entry);
        }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn peek(&self, direction: Direction) -> Option<&Entry> {
        match direction {
            Direction::Undo => self.undo.last(),
            Direction::Redo => self.redo.last(),
        }
    }

    pub fn take(&mut self, direction: Direction) -> Option<Entry> {
        self.stack(direction).pop()
    }

    pub fn finish(&mut self, direction: Direction, report: &mut Report) {
        // Re-trashing a restored file creates a new platform id. Rebind only
        // that exact old record, preserving unrelated same-path deletions.
        for replacement in &report.replacements {
            for step in self
                .undo
                .iter_mut()
                .chain(self.redo.iter_mut())
                .flat_map(|entry| &mut entry.steps)
                .chain(report.remaining.iter_mut())
                .chain(report.inverse.iter_mut())
            {
                match step {
                    Step::Restore(entry) if entry.id == replacement.previous => {
                        *entry = replacement.current.clone()
                    }
                    Step::Trash {
                        previous: Some(id), ..
                    } if *id == replacement.previous => *id = replacement.current.id.clone(),
                    _ => {}
                }
            }
        }
        if !report.remaining.is_empty() {
            self.stack(direction).push(Entry::new(
                report.kind,
                std::mem::take(&mut report.remaining),
            ));
        }
        if !report.inverse.is_empty() {
            let opposite = match direction {
                Direction::Undo => Direction::Redo,
                Direction::Redo => Direction::Undo,
            };
            self.stack(opposite)
                .push(Entry::new(report.kind, std::mem::take(&mut report.inverse)));
        }
    }

    fn stack(&mut self, direction: Direction) -> &mut Vec<Entry> {
        match direction {
            Direction::Undo => &mut self.undo,
            Direction::Redo => &mut self.redo,
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
