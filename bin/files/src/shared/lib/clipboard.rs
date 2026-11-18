//! File clipboard interchange through the application window's native selection.
//! Reads snapshot all file representations; writes can require an unchanged offer.

mod protocol;
mod state;
mod wayland;

use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

use anti_webui::prelude::*;
use gds::app::Gds;
use protocol::Formats;
use serde::{Deserialize, Serialize};
use state::Cache;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Copy,
    Cut,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity(Option<String>);

impl Identity {
    fn fresh() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(1);
        Self(Some(format!(
            "gds:{}:{:?}:{}",
            std::process::id(),
            SystemTime::now(),
            SERIAL.fetch_add(1, Ordering::Relaxed),
        )))
    }

    fn is_owned(&self) -> bool {
        self.0.is_some()
    }

    /// # Errors
    /// Rejects an empty or non-text clipboard identity.
    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let value = std::str::from_utf8(bytes)
            .map_err(|_| Error::InvalidPayload("clipboard identity must be UTF-8".into()))?;
        if value.is_empty() {
            return Err(Error::InvalidPayload("clipboard identity is empty".into()));
        }
        Ok(Self(Some(value.to_owned())))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub mode: Mode,
    pub paths: Vec<PathBuf>,
    pub identity: Identity,
}

impl Snapshot {
    pub fn can_cancel(&self) -> bool {
        self.mode == Mode::Cut && self.identity.is_owned()
    }

    /// # Errors
    /// Rejects empty selections and paths that are not absolute named filesystem entries.
    fn new(mode: Mode, paths: Vec<PathBuf>) -> Result<Self, Error> {
        let paths = Self::validate_paths(paths)?;
        Ok(Self {
            mode,
            paths,
            identity: Identity::fresh(),
        })
    }

    /// # Errors
    /// Rejects empty or non-local filesystem entry paths.
    fn validate_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, Error> {
        let mut seen = HashSet::new();
        let mut unique = Vec::with_capacity(paths.len());
        for path in paths {
            if !path.is_absolute()
                || !matches!(path.components().next_back(), Some(Component::Normal(_)))
                || path.as_os_str().as_encoded_bytes().contains(&0)
            {
                return Err(Error::InvalidPath(path));
            }
            if seen.insert(path.clone()) {
                unique.push(path);
            }
        }
        if unique.is_empty() {
            return Err(Error::EmptySelection);
        }
        Ok(unique)
    }

    fn excluding(&self, current: &Self, removed: &[PathBuf]) -> Option<Vec<PathBuf>> {
        if self != current {
            return None;
        }
        let removed: HashSet<&Path> = removed.iter().map(PathBuf::as_path).collect();
        let remaining: Vec<_> = self
            .paths
            .iter()
            .filter(|path| !removed.contains(path.as_path()))
            .cloned()
            .collect();
        (remaining.len() != self.paths.len()).then_some(remaining)
    }

    fn remaining_after(&self, current: &Self, succeeded: &[PathBuf]) -> Option<Vec<PathBuf>> {
        // Foreign offers have no per-copy identity. Do not overwrite their owner,
        // even if another copy happens to contain exactly the same path list.
        if self.mode != Mode::Cut || !self.identity.is_owned() || self != current {
            return None;
        }
        let completed: HashSet<&Path> = succeeded.iter().map(PathBuf::as_path).collect();
        let remaining: Vec<_> = self
            .paths
            .iter()
            .filter(|path| !completed.contains(path.as_path()))
            .cloned()
            .collect();
        (remaining.len() != self.paths.len()).then_some(remaining)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no files are selected")]
    EmptySelection,
    #[error("clipboard path is not an absolute named filesystem entry: {0}")]
    InvalidPath(PathBuf),
    #[error("clipboard contains an unsupported or non-local file URI: {0}")]
    InvalidUri(String),
    #[error("invalid file clipboard data: {0}")]
    InvalidPayload(String),
    #[error("file clipboard payload exceeds the supported size")]
    TooLarge,
    #[error("file clipboard I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("file clipboard data could not be encoded or decoded: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Native(#[from] ClipboardError),
}

thread_local! { static INSTALLED: RefCell<Option<Clipboard>> = const { RefCell::new(None) }; }
pub(crate) enum Payload {
    Empty,
    Local(Snapshot),
    Uris { mode: Mode, uris: Vec<String> },
}
#[derive(Clone)]
pub(crate) struct Clipboard(Rc<ClipboardState>);
struct ClipboardState {
    cache: RefCell<Cache>,
    revision: Signal<u64>,
}
impl Clipboard {
    pub fn current() -> Self {
        if let Some(clipboard) = INSTALLED.with(|slot| slot.borrow().clone()) {
            return clipboard;
        }
        Gds::current().with(|| {
            let clipboard = Self(Rc::new(ClipboardState {
                cache: RefCell::new(Cache::default()),
                revision: signal(0),
            }));
            INSTALLED.with(|slot| *slot.borrow_mut() = Some(clipboard.clone()));
            on_cleanup(|| {
                INSTALLED.with(|slot| slot.borrow_mut().take());
            });
            clipboard
        })
    }
    fn changed(&self) {
        self.0
            .revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }
    pub fn snapshot(&self) -> Option<Arc<Snapshot>> {
        self.0.revision.get();
        self.0.cache.borrow().current()
    }
    pub fn is_cut(&self, path: &Path) -> bool {
        self.0.revision.get();
        self.0.cache.borrow().is_cut(path)
    }
    pub fn can_paste(&self) -> bool {
        self.0.revision.get();
        self.0.cache.borrow().can_paste()
    }
    pub fn refresh(&self) {
        let Some(generation) = self.0.cache.borrow_mut().begin_refresh() else {
            return;
        };
        let pending = Rc::new(std::cell::Cell::new(true));
        let cleanup = pending.clone();
        let clipboard = self.clone();
        on_cleanup(move || {
            if cleanup.replace(false) {
                clipboard.0.cache.borrow_mut().reconcile(
                    generation,
                    Err(Error::InvalidPayload("requesting window closed".into())),
                    false,
                );
            }
        });
        let clipboard = self.clone();
        wayland::Clipboard::offer().then(move |result| {
            pending.set(false);
            let result = result.map_err(Error::from).and_then(|offer| {
                let remote =
                    wayland::Clipboard::decode_uris(&offer)?.is_some_and(|(mode, uris)| {
                        mode == Mode::Copy && Formats::copyable_uris(&uris)
                    });
                match wayland::Clipboard::decode(&offer) {
                    Ok(snapshot) => Ok((snapshot, remote)),
                    Err(Error::InvalidUri(_)) => Ok((None, remote)),
                    Err(error) => Err(error),
                }
            });
            let (result, remote) = match result {
                Ok((snapshot, remote)) => (Ok(snapshot), remote),
                Err(error) => (Err(error), false),
            };
            let changed = clipboard
                .0
                .cache
                .borrow_mut()
                .reconcile(generation, result, remote);
            if changed {
                clipboard.changed();
            }
        });
    }
    pub fn write(&self, mode: Mode, paths: Vec<PathBuf>) -> Task<Result<Snapshot, Error>> {
        let snapshot = match Snapshot::new(mode, paths) {
            Ok(snapshot) => snapshot,
            Err(error) => return Task::ready(Err(error)),
        };
        let clipboard = self.clone();
        wayland::Clipboard::write(snapshot).map(move |result| {
            if let Ok(snapshot) = &result {
                clipboard.0.cache.borrow_mut().record_write(snapshot);
                clipboard.changed();
            }
            result
        })
    }
    #[expect(
        dead_code,
        reason = "Typed local clipboard reads remain available beside mixed URI payload reads"
    )]
    pub fn read(&self) -> Task<Result<Option<Snapshot>, Error>> {
        wayland::Clipboard::read()
    }
    pub fn read_payload(&self) -> Task<Result<Payload, Error>> {
        wayland::Clipboard::offer().map(|offer| {
            let offer = offer?;
            match wayland::Clipboard::decode(&offer) {
                Ok(Some(snapshot)) => Ok(Payload::Local(snapshot)),
                Ok(None) => Ok(Payload::Empty),
                Err(Error::InvalidUri(_)) => Ok(match wayland::Clipboard::decode_uris(&offer)? {
                    Some((mode, uris)) => Payload::Uris { mode, uris },
                    None => Payload::Empty,
                }),
                Err(error) => Err(error),
            }
        })
    }
    pub fn remove(&self, expected: Arc<Snapshot>, paths: Vec<PathBuf>) -> Task<Result<(), Error>> {
        let clipboard = self.clone();
        wayland::Clipboard::remove_paths(expected, paths, true).map(move |result| {
            clipboard.refresh();
            result
        })
    }
    pub fn read_uris(&self) -> Task<Result<Option<(Mode, Vec<String>)>, Error>> {
        wayland::Clipboard::read_uris()
    }
    pub fn write_uris(&self, uris: Vec<String>) -> Task<Result<(), Error>> {
        let clipboard = self.clone();
        wayland::Clipboard::write_uris(uris).map(move |result| {
            if result.is_ok() {
                clipboard.0.cache.borrow_mut().record_uri_copy(true);
                clipboard.changed();
            }
            result
        })
    }
    pub fn finish_cut(
        &self,
        expected: Snapshot,
        succeeded: Vec<PathBuf>,
    ) -> Task<Result<(), Error>> {
        let clipboard = self.clone();
        let snapshot = expected.clone();
        let removed = succeeded.clone();
        wayland::Clipboard::remove_paths(Arc::new(expected), succeeded, false).map(move |result| {
            if result.is_ok() {
                clipboard
                    .0
                    .cache
                    .borrow_mut()
                    .note_pasted(&snapshot, &removed);
                clipboard.changed();
                clipboard.refresh();
            }
            result
        })
    }
    pub fn cancel_cut(&self) -> Option<Task<Result<(), Error>>> {
        let expected = self.0.cache.borrow_mut().begin_cancel()?;
        let pending = Rc::new(std::cell::Cell::new(true));
        let cleanup = pending.clone();
        let clipboard = self.clone();
        let snapshot = expected.clone();
        on_cleanup(move || {
            if cleanup.replace(false) {
                clipboard
                    .0
                    .cache
                    .borrow_mut()
                    .finish_cancel(&snapshot, false);
            }
        });
        let clipboard = self.clone();
        let snapshot = expected.clone();
        Some(
            wayland::Clipboard::remove_paths(expected.clone(), expected.paths.clone(), false).map(
                move |result| {
                    pending.set(false);
                    clipboard
                        .0
                        .cache
                        .borrow_mut()
                        .finish_cancel(&snapshot, result.is_ok());
                    clipboard.changed();
                    clipboard.refresh();
                    result
                },
            ),
        )
    }
}
#[cfg(test)]
mod tests;
