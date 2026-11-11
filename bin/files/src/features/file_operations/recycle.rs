//! Recycle-bin records backed by the platform trash store.
//!
//! Linux uses the freedesktop trash through the `trash` crate. Every [`Entry`]
//! keeps the raw record id plus the resolved info/payload paths and file
//! identities captured at listing time, so an action only ever touches the
//! records the caller froze.

use std::{ffi::OsString, io, path::PathBuf, time::SystemTime};

#[cfg(target_os = "linux")]
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs,
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

use crate::entities::file::directory::EntryKind;
use crate::features::file_operations::transfer;

#[cfg(target_os = "linux")]
use crate::features::file_operations::{
    names::{self, NamedTarget},
    transfer::{Batch, Mode, Request},
};

/// Whether this build can browse and act on the platform trash store.
pub fn supported() -> bool {
    cfg!(target_os = "linux")
}

/// One record currently in the trash.
#[derive(Clone, Debug)]
pub struct Entry {
    /// Raw platform record identifier; also the selection key in the UI.
    pub id: OsString,
    /// Name the record had at its original location.
    pub name: OsString,
    /// Original location, joined from the record's parent and name.
    pub original_path: PathBuf,
    /// Deletion time when the record carries a usable timestamp.
    pub deleted_at: Option<SystemTime>,
    /// File length, or `None` when the file does not report one.
    pub size: Option<u64>,
    /// Direct child count for a directory.
    pub items: Option<usize>,
    /// Entry type captured with `lstat`; a link is never reported as its target.
    pub kind: EntryKind,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    info: PathBuf,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    payload: PathBuf,
    /// Parent of the original location, validated absolute at listing time.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    original_parent: PathBuf,
    #[cfg(target_os = "linux")]
    snapshot: Snapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Restore,
    Purge,
}

#[derive(Debug)]
pub struct Outcome {
    pub entry: Entry,
    pub original_path: PathBuf,
    pub result: Result<(), Error>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
    #[error("operation cancelled")]
    Cancelled,
    #[error(
        "the file was moved to trash, but its unique recovery record could not be identified: {path}"
    )]
    MissingReceipt { path: PathBuf },
    #[error("the recycle bin is only supported on Linux")]
    #[cfg(not(target_os = "linux"))]
    Unsupported,
    #[error(transparent)]
    Trash(#[from] trash::Error),
    #[error("not a valid freedesktop trash record: {path}")]
    InvalidRecord { path: PathBuf },
    #[error("the trash record changed or disappeared since it was listed: {path}")]
    Stale { path: PathBuf },
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("the original location already holds an entry; nothing was overwritten: {path}")]
    RestoreConflict { path: PathBuf },
    #[error("none of the {failed} trash records could be read; first failure: {first}")]
    Unreadable { failed: usize, first: Box<Error> },
    #[error(transparent)]
    Transfer(#[from] transfer::Error),
    #[error("{path} was restored, but its trash record could not be cleaned up: {source}")]
    RestoreIncomplete {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "{path} was permanently deleted, but its trash record could not be cleaned up: {source}"
    )]
    PurgeIncomplete {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// A deletion can succeed even if reading its recovery record subsequently fails.
pub struct Receipt {
    pub entry: Result<Entry, Error>,
}

/// Tracks pre-existing trash IDs once across a batch of deletions so that
/// `trash::os_limited::list()` is executed at most once before and once after
/// the batch instead of $2N$ times.
#[derive(Default)]
pub(crate) struct Tracker {
    #[cfg(target_os = "linux")]
    before: Option<HashSet<OsString>>,
}

impl Tracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Delete one path to trash, lazily snapshotting pre-existing trash IDs on
    /// the first deletion in the batch.
    ///
    /// # Errors
    /// Reports cancellation, initial snapshot listing failure, path resolution
    /// failure, or deletion failure.
    pub(crate) fn delete(
        &mut self,
        path: &std::path::Path,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<PathBuf, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        #[cfg(target_os = "linux")]
        {
            if self.before.is_none() {
                let ids = trash::os_limited::list()?
                    .into_iter()
                    .map(|entry| entry.id)
                    .collect();
                self.before = Some(ids);
            }
            let original = Self::resolve_original(path)?;
            trash::delete(path)?;
            Ok(original)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = path;
            Err(Error::Unsupported)
        }
    }

    /// Resolve recovery [`Entry`] records for all paths deleted in this batch
    /// with a single post-batch `trash::os_limited::list()` scan.
    pub(crate) fn finish(self, originals: Vec<PathBuf>) -> Vec<Result<Entry, Error>> {
        if originals.is_empty() {
            return Vec::new();
        }
        #[cfg(target_os = "linux")]
        {
            let Some(before) = self.before else {
                return originals
                    .into_iter()
                    .map(|path| Err(Error::MissingReceipt { path }))
                    .collect();
            };
            let entries = match trash::os_limited::list() {
                Ok(entries) => entries,
                Err(error) => {
                    let mut iter = originals.into_iter();
                    let mut results = Vec::with_capacity(iter.len());
                    if iter.next().is_some() {
                        results.push(Err(Error::from(error)));
                    }
                    results.extend(iter.map(|path| Err(Error::MissingReceipt { path })));
                    return results;
                }
            };
            let mut expected_counts: HashMap<PathBuf, usize> = HashMap::new();
            for original in &originals {
                *expected_counts.entry(original.clone()).or_insert(0) += 1;
            }
            let mut by_original: HashMap<PathBuf, Vec<trash::TrashItem>> = HashMap::new();
            for item in entries {
                if !before.contains(&item.id) {
                    by_original
                        .entry(item.original_path())
                        .or_default()
                        .push(item);
                }
            }
            for (original, items) in &mut by_original {
                if expected_counts.get(original).copied().unwrap_or(0) != items.len() {
                    items.clear();
                } else {
                    items.reverse();
                }
            }
            originals
                .into_iter()
                .map(|original| {
                    by_original
                        .get_mut(&original)
                        .and_then(Vec::pop)
                        .ok_or_else(|| Error::MissingReceipt {
                            path: original.clone(),
                        })
                        .and_then(Entry::from_item)
                })
                .collect()
        }
        #[cfg(not(target_os = "linux"))]
        {
            originals
                .into_iter()
                .map(|_| Err(Error::Unsupported))
                .collect()
        }
    }

    #[cfg(target_os = "linux")]
    fn resolve_original(path: &Path) -> Result<PathBuf, Error> {
        Ok(path
            .parent()
            .ok_or_else(|| Error::InvalidRecord {
                path: path.to_owned(),
            })?
            .canonicalize()
            .map_err(|source| Error::Io {
                path: path.to_owned(),
                source,
            })?
            .join(path.file_name().ok_or_else(|| Error::InvalidRecord {
                path: path.to_owned(),
            })?))
    }
}

/// Move one file to trash with cancellation support and capture its exact new
/// record, not an older deletion with the same original path.
///
/// # Errors
/// Reports cancellation, listing, path resolution or deletion failure before a receipt exists.
pub fn trash_cancellable(
    path: &std::path::Path,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<Receipt, Error> {
    let mut tracker = Tracker::new();
    let original = tracker.delete(path, cancel)?;
    let entry = tracker
        .finish(vec![original.clone()])
        .pop()
        .unwrap_or(Err(Error::MissingReceipt { path: original }));
    Ok(Receipt { entry })
}

/// List every trash record that can still be acted on.
///
/// Records whose layout or payload no longer matches the freedesktop layout are
/// skipped and reported on stderr, so one corrupt record cannot hide the rest.
/// An enumeration that yields records without a single readable one is an
/// error, never an empty trash.
///
/// # Errors
/// Reports a platform that has no supported trash backend, a failure to
/// enumerate the trash folders, or a listing where every record was rejected.
pub fn list() -> Result<Vec<Entry>, Error> {
    #[cfg(target_os = "linux")]
    {
        Entry::collect(trash::os_limited::list()?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::Unsupported)
    }
}

/// Apply one action to exactly the supplied records with cancellation support,
/// keeping every outcome.
///
/// The outcome list preserves the input order, and one record's failure never
/// stops the remaining records from being attempted.
pub fn perform_cancellable(
    action: Action,
    entries: Vec<Entry>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Vec<Outcome> {
    entries
        .into_iter()
        .map(|entry| {
            let original_path = entry.original_path.clone();
            Outcome {
                original_path,
                result: if cancel.is_cancelled() {
                    Err(Error::Cancelled)
                } else {
                    entry.apply(action, cancel)
                },
                entry,
            }
        })
        .collect()
}

impl Entry {
    /// Build entries from raw records, keeping the readable ones and remembering
    /// the first rejection. Failing only because some records are corrupt would
    /// hide the usable ones; succeeding with none of them would fake an empty bin.
    ///
    /// # Errors
    /// Reports [`Error::Unreadable`] when items exist but every single one is invalid.
    #[cfg(target_os = "linux")]
    fn collect(items: impl IntoIterator<Item = trash::TrashItem>) -> Result<Vec<Self>, Error> {
        let mut entries = Vec::new();
        let mut failures = 0;
        let mut first_failure = None;
        for item in items {
            match Self::from_item(item) {
                Ok(entry) => entries.push(entry),
                Err(error) => {
                    tracing::warn!("Skipping unusable trash record: {error}");
                    failures += 1;
                    first_failure.get_or_insert(error);
                }
            }
        }
        match first_failure {
            Some(first) if entries.is_empty() => Err(Error::Unreadable {
                failed: failures,
                first: Box::new(first),
            }),
            _ => Ok(entries),
        }
    }

    /// # Errors
    /// Propagates restore or purge failures for this record.
    fn apply(
        &self,
        action: Action,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(), Error> {
        #[cfg(target_os = "linux")]
        {
            match action {
                Action::Restore => self.restore(cancel),
                Action::Purge => self.purge(),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (action, cancel);
            Err(Error::Unsupported)
        }
    }

    /// # Errors
    /// Refuses stale records, unavailable destination parents, an occupied
    /// original location, and reports a delivered payload whose record cleanup
    /// failed without rolling the restored payload back.
    #[cfg(target_os = "linux")]
    fn restore(&self, cancel: &tokio_util::sync::CancellationToken) -> Result<(), Error> {
        self.verify()?;
        fs::create_dir_all(&self.original_parent).map_err(|source| Error::Io {
            path: self.original_parent.clone(),
            source,
        })?;
        let request = Request {
            paths: vec![self.payload.clone()],
            destination: self.original_parent.clone(),
            mode: Mode::Move,
        };
        let targets = vec![NamedTarget::new(self.payload.clone(), self.name.clone())];
        let batch = Batch::named(request, targets)?;
        let mut items = batch.execute_cancellable(cancel.clone()).items;
        let item = items
            .pop()
            .expect("a named batch retains its single frozen payload");
        if item.destination_committed {
            if let Err(error) = item.result {
                return Err(Error::RestoreIncomplete {
                    path: self.original_path.clone(),
                    source: io::Error::other(error),
                });
            }
        } else if let Err(error) = item.result {
            return Err(match error {
                transfer::Error::Cancelled => Error::Cancelled,
                transfer::Error::Conflict { path } => Error::RestoreConflict { path },
                other => Error::Transfer(other),
            });
        }
        fs::remove_file(&self.info).map_err(|source| Error::RestoreIncomplete {
            path: self.original_path.clone(),
            source,
        })
    }

    /// # Errors
    /// Refuses stale records and reports removal failures. A payload is only
    /// removed after the action starts, and its info file is only removed once
    /// the payload is gone.
    #[cfg(target_os = "linux")]
    fn purge(&self) -> Result<(), Error> {
        let metadata = self.verify()?;
        let removal = if metadata.is_dir() {
            fs::remove_dir_all(&self.payload)
        } else {
            fs::remove_file(&self.payload)
        };
        removal.map_err(|source| Error::Io {
            path: self.payload.clone(),
            source,
        })?;
        fs::remove_file(&self.info).map_err(|source| Error::PurgeIncomplete {
            path: self.original_path.clone(),
            source,
        })
    }

    /// Reject a record whose info or payload was replaced since it was listed.
    ///
    /// # Errors
    /// Reports missing/replaced records as [`Error::Stale`] and I/O failures.
    #[cfg(target_os = "linux")]
    fn verify(&self) -> Result<fs::Metadata, Error> {
        let info = Self::inspect(&self.info)?;
        if Identity::of(&info) != self.snapshot.info {
            return Err(Error::Stale {
                path: self.info.clone(),
            });
        }
        let payload = Self::inspect(&self.payload)?;
        if Identity::of(&payload) != self.snapshot.payload {
            return Err(Error::Stale {
                path: self.payload.clone(),
            });
        }
        Ok(payload)
    }

    /// # Errors
    /// Rejects ids that are not absolute `.trashinfo` leaves under an `info`
    /// directory, a relative original parent, a missing payload or info file,
    /// and a non-file info leaf.
    #[cfg(target_os = "linux")]
    fn from_item(item: trash::TrashItem) -> Result<Self, Error> {
        let paths = RecordPaths::parse(&item.id)?;
        if !item.original_parent.is_absolute() || names::validate(&item.name).is_err() {
            return Err(Error::InvalidRecord { path: paths.info });
        }
        let payload_metadata = Self::inspect(&paths.payload)?;
        let info_metadata = Self::inspect(&paths.info)?;
        if !info_metadata.file_type().is_file() {
            return Err(Error::InvalidRecord { path: paths.info });
        }
        let original_path = item.original_path();
        let deleted_at = Self::deleted_at(item.time_deleted);
        Ok(Self {
            id: item.id,
            name: item.name,
            original_path,
            deleted_at,
            size: payload_metadata.is_file().then(|| payload_metadata.len()),
            items: None,
            kind: EntryKind::from(payload_metadata.file_type()),
            snapshot: Snapshot {
                info: Identity::of(&info_metadata),
                payload: Identity::of(&payload_metadata),
            },
            info: paths.info,
            payload: paths.payload,
            original_parent: item.original_parent,
        })
    }

    /// # Errors
    /// Maps `NotFound` to [`Error::Stale`] and other errors to [`Error::Io`].
    #[cfg(target_os = "linux")]
    fn inspect(path: &Path) -> Result<fs::Metadata, Error> {
        fs::symlink_metadata(path).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                Error::Stale {
                    path: path.to_owned(),
                }
            } else {
                Error::Io {
                    path: path.to_owned(),
                    source,
                }
            }
        })
    }

    #[cfg(target_os = "linux")]
    fn deleted_at(seconds: i64) -> Option<SystemTime> {
        u64::try_from(seconds)
            .ok()
            .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
    }
}

/// Info and payload locations derived from one raw trash id.
#[cfg(target_os = "linux")]
struct RecordPaths {
    info: PathBuf,
    payload: PathBuf,
}

#[cfg(target_os = "linux")]
impl RecordPaths {
    /// # Errors
    /// Rejects non-absolute paths, non-`.trashinfo` extensions, invalid stems,
    /// or paths not directly inside `<trash_root>/info/`.
    fn parse(id: &OsStr) -> Result<Self, Error> {
        let info = PathBuf::from(id);
        if !info.is_absolute() {
            return Err(Self::invalid(&info));
        }
        let Some(file_name) = info.file_name() else {
            return Err(Self::invalid(&info));
        };
        let file_name = Path::new(file_name);
        if file_name.extension() != Some(OsStr::new("trashinfo")) {
            return Err(Self::invalid(&info));
        }
        let Some(stem) = file_name.file_stem() else {
            return Err(Self::invalid(&info));
        };
        if names::validate(stem).is_err() {
            return Err(Self::invalid(&info));
        }
        let Some(info_dir) = info.parent() else {
            return Err(Self::invalid(&info));
        };
        if info_dir.file_name() != Some(OsStr::new("info")) {
            return Err(Self::invalid(&info));
        }
        let Some(root) = info_dir.parent() else {
            return Err(Self::invalid(&info));
        };
        let payload = root.join("files").join(stem);
        Ok(Self { info, payload })
    }

    fn invalid(info: &Path) -> Error {
        Error::InvalidRecord {
            path: info.to_owned(),
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    length: u64,
    modified: i64,
    modified_nanos: i64,
}

#[cfg(target_os = "linux")]
impl Identity {
    fn of(metadata: &fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt as _;

        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified: metadata.mtime(),
            modified_nanos: metadata.mtime_nsec(),
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
struct Snapshot {
    info: Identity,
    payload: Identity,
}
