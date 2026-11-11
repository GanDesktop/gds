//! Recycle-bin records backed by the platform trash store.
//!
//! Linux uses the freedesktop trash through the `trash` crate. Every [`Entry`]
//! keeps the raw record id plus the resolved info/payload paths and file
//! identities captured at listing time, so an action only ever touches the
//! records the caller froze.

use std::{ffi::OsString, io, path::PathBuf, time::SystemTime};

#[cfg(target_os = "linux")]
use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path},
    time::{Duration, UNIX_EPOCH},
};

use crate::entities::file::directory::EntryKind;
use crate::features::file_operations::transfer;

#[cfg(target_os = "linux")]
use crate::features::file_operations::{
    names::NamedTarget,
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

/// Move one file to trash and capture its exact new record, not an older
/// deletion with the same original path.
///
/// # Errors
/// Reports listing, path resolution or deletion failure before a receipt exists.
#[cfg(target_os = "linux")]
pub fn trash(path: &std::path::Path) -> Result<Receipt, Error> {
    let before: std::collections::HashSet<_> = trash::os_limited::list()?
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    let original = path
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
        })?);
    trash::delete(path)?;
    let entry = trash::os_limited::list()
        .map_err(Error::from)
        .and_then(|entries| {
            let mut matches = entries
                .into_iter()
                .filter(|entry| entry.original_path() == original && !before.contains(&entry.id));
            match (matches.next(), matches.next()) {
                (Some(entry), None) => Entry::from_item(entry),
                _ => Err(Error::MissingReceipt { path: original }),
            }
        });
    Ok(Receipt { entry })
}

#[cfg(not(target_os = "linux"))]
pub fn trash(_: &std::path::Path) -> Result<Receipt, Error> {
    Err(Error::Unsupported)
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
        collect_entries(trash::os_limited::list()?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::Unsupported)
    }
}

/// Build entries from raw records, keeping the readable ones and remembering
/// the first rejection. Failing only because some records are corrupt would
/// hide the usable ones; succeeding with none of them would fake an empty bin.
#[cfg(target_os = "linux")]
fn collect_entries(items: impl IntoIterator<Item = trash::TrashItem>) -> Result<Vec<Entry>, Error> {
    let mut entries = Vec::new();
    let mut failures = 0;
    let mut first_failure = None;
    for item in items {
        match Entry::from_item(item) {
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

/// Apply one action to exactly the supplied records, keeping every outcome.
///
/// The outcome list preserves the input order, and one record's failure never
/// stops the remaining records from being attempted.
pub fn perform(action: Action, entries: Vec<Entry>) -> Vec<Outcome> {
    perform_cancellable(action, entries, &tokio_util::sync::CancellationToken::new())
}

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
                    entry.apply(action)
                },
                entry,
            }
        })
        .collect()
}

impl Entry {
    fn apply(&self, action: Action) -> Result<(), Error> {
        #[cfg(target_os = "linux")]
        {
            match action {
                Action::Restore => self.restore(),
                Action::Purge => self.purge(),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = action;
            Err(Error::Unsupported)
        }
    }

    /// # Errors
    /// Refuses stale records, unavailable destination parents, an occupied
    /// original location, and reports a delivered payload whose record cleanup
    /// failed without rolling the restored payload back.
    #[cfg(target_os = "linux")]
    fn restore(&self) -> Result<(), Error> {
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
        let mut items = batch.execute().items;
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
    #[cfg(target_os = "linux")]
    fn verify(&self) -> Result<fs::Metadata, Error> {
        let info = inspect(&self.info)?;
        if Identity::of(&info) != self.snapshot.info {
            return Err(Error::Stale {
                path: self.info.clone(),
            });
        }
        let payload = inspect(&self.payload)?;
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
        if !item.original_parent.is_absolute() || !is_leaf(&item.name) {
            return Err(Error::InvalidRecord { path: paths.info });
        }
        let payload_metadata = inspect(&paths.payload)?;
        let info_metadata = inspect(&paths.info)?;
        if !info_metadata.file_type().is_file() {
            return Err(Error::InvalidRecord { path: paths.info });
        }
        let detail = trash::os_limited::metadata(&item).ok();
        let original_path = item.original_path();
        let deleted_at = deleted_at(item.time_deleted);
        Ok(Self {
            id: item.id,
            name: item.name,
            original_path,
            deleted_at,
            size: detail.and_then(|detail| detail.size.size()),
            items: detail.and_then(|detail| detail.size.entries()),
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
}

/// Info and payload locations derived from one raw trash id.
#[cfg(target_os = "linux")]
struct RecordPaths {
    info: PathBuf,
    payload: PathBuf,
}

#[cfg(target_os = "linux")]
impl RecordPaths {
    fn parse(id: &OsStr) -> Result<Self, Error> {
        let info = PathBuf::from(id);
        if !info.is_absolute() {
            return Err(invalid_record(&info));
        }
        let Some(file_name) = info.file_name() else {
            return Err(invalid_record(&info));
        };
        let file_name = Path::new(file_name);
        if file_name.extension() != Some(OsStr::new("trashinfo")) {
            return Err(invalid_record(&info));
        }
        let Some(stem) = file_name.file_stem() else {
            return Err(invalid_record(&info));
        };
        if !is_leaf(stem) {
            return Err(invalid_record(&info));
        }
        let Some(info_dir) = info.parent() else {
            return Err(invalid_record(&info));
        };
        if info_dir.file_name() != Some(OsStr::new("info")) {
            return Err(invalid_record(&info));
        }
        let Some(root) = info_dir.parent() else {
            return Err(invalid_record(&info));
        };
        let payload = root.join("files").join(stem);
        Ok(Self { info, payload })
    }
}

#[cfg(target_os = "linux")]
fn invalid_record(info: &Path) -> Error {
    Error::InvalidRecord {
        path: info.to_owned(),
    }
}

/// Whether `name` is exactly one normal path component.
#[cfg(target_os = "linux")]
fn is_leaf(name: &OsStr) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

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
