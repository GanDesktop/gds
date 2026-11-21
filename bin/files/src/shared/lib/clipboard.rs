//! File clipboard interchange via `zgui`'s multi-MIME clipboard (`read_mime` / `write_mime`),
//! backed by an in-process `Snapshot` cache and unfocused fallback for headless/Weston runs.

mod native;
mod protocol;
mod state;

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    path::{Component, Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

use gds::app::Gds;
use native::NativeClipboard;
use protocol::Formats;
use serde::{Deserialize, Serialize};
use state::Cache;
use zgui::platform::{ClipboardError, ClipboardMimeOffer};
use zgui::prelude::{ClipboardKind, LocalStorage, RwSignal, try_use_clipboard};
use zgui::reactive::{on_cleanup_local, prelude::*, spawn_local};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Copy,
    Cut,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity(pub(super) Option<String>);

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
    Platform(#[from] ClipboardError),
}

thread_local! {
    static INSTALLED: RefCell<Option<Clipboard>> = const { RefCell::new(None) };
}

pub(crate) enum Payload {
    Empty,
    Local(Snapshot),
    Uris { mode: Mode, uris: Vec<String> },
}

#[derive(Clone)]
pub(crate) struct Clipboard(Rc<ClipboardState>);

struct ClipboardState {
    cache: RefCell<Cache>,
    write_seq: Cell<u64>,
    revision: RwSignal<u64, LocalStorage>,
}

struct RefreshGuard {
    weak: std::rc::Weak<ClipboardState>,
    generation: u64,
    completed: bool,
}

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        if !self.completed
            && let Some(state) = self.weak.upgrade()
        {
            state
                .cache
                .borrow_mut()
                .finish_refresh_unchanged(self.generation);
        }
    }
}

fn should_use_local_fallback(error: &ClipboardError) -> bool {
    matches!(
        error,
        ClipboardError::NotFocused(_)
            | ClipboardError::UnsupportedFormat(_)
            | ClipboardError::UnsupportedKind(_)
    )
}

fn select_remaining(
    expected: &Snapshot,
    current: &Snapshot,
    removed: &[PathBuf],
    explicit: bool,
) -> Option<Vec<PathBuf>> {
    if explicit {
        expected.excluding(current, removed)
    } else {
        expected.remaining_after(current, removed)
    }
}

impl Clipboard {
    pub fn current() -> Self {
        if let Some(clipboard) = INSTALLED.with(|slot| slot.borrow().clone()) {
            return clipboard;
        }
        let install = || {
            let clipboard = Self(Rc::new(ClipboardState {
                cache: RefCell::new(Cache::default()),
                write_seq: Cell::new(0),
                revision: RwSignal::new_local(0),
            }));
            INSTALLED.with(|slot| *slot.borrow_mut() = Some(clipboard.clone()));
            on_cleanup_local(|| {
                INSTALLED.with(|slot| slot.borrow_mut().take());
            });
            clipboard
        };
        match Gds::try_current() {
            Some(gds) => gds.with(install),
            None => install(),
        }
    }

    fn changed(&self) {
        self.0
            .revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    fn next_write_seq(&self) -> u64 {
        let next = self.0.write_seq.get().wrapping_add(1);
        self.0.write_seq.set(next);
        next
    }

    pub fn snapshot(&self) -> Option<Arc<Snapshot>> {
        self.0.revision.get();
        self.0.cache.borrow().current()
    }

    pub fn snapshot_untracked(&self) -> Option<Arc<Snapshot>> {
        self.0.cache.borrow().current()
    }

    pub fn is_cut(&self, path: &Path) -> bool {
        self.0.revision.get();
        self.0.cache.borrow().is_cut(path)
    }

    pub fn can_paste_untracked(&self) -> bool {
        self.0.cache.borrow().can_paste()
    }

    pub fn refresh(&self) {
        let Some(clip) = try_use_clipboard() else {
            return;
        };
        let Some(generation) = self.0.cache.borrow_mut().begin_refresh() else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        let cancel = Gds::try_current().map(|gds| gds.tasks().scope().token());
        let task_body = async move {
            let mut guard = RefreshGuard {
                weak: weak.clone(),
                generation,
                completed: false,
            };
            let offer_result = NativeClipboard::read_offer(&clip).await;
            let Some(state) = weak.upgrade() else {
                guard.completed = true;
                return;
            };
            if cancel.as_ref().is_some_and(|token| token.is_cancelled()) {
                return;
            }
            let clipboard = Clipboard(state);
            let changed = match offer_result {
                Ok(opt_offer) => {
                    let observed_rev = opt_offer.as_ref().map(|offer| offer.revision);
                    match opt_offer.as_ref().map(Self::interpret_offer) {
                        Some(Ok((snapshot, remote))) if snapshot.is_some() || remote => clipboard
                            .0
                            .cache
                            .borrow_mut()
                            .reconcile(generation, Ok(snapshot), remote),
                        Some(Err(error)) => {
                            clipboard
                                .0
                                .cache
                                .borrow_mut()
                                .reconcile(generation, Err(error), false)
                        }
                        None | Some(Ok((None, false))) => {
                            let preserve = clipboard
                                .0
                                .cache
                                .borrow()
                                .fallback_matches_platform(observed_rev);
                            if preserve {
                                clipboard
                                    .0
                                    .cache
                                    .borrow_mut()
                                    .finish_refresh_unchanged(generation);
                                false
                            } else {
                                clipboard.0.cache.borrow_mut().reconcile(
                                    generation,
                                    Ok(None),
                                    false,
                                )
                            }
                        }
                        Some(Ok(_)) => unreachable!(),
                    }
                }
                Err(error) => clipboard.0.cache.borrow_mut().reconcile(
                    generation,
                    Err(Error::Platform(error)),
                    false,
                ),
            };
            guard.completed = true;
            if changed {
                clipboard.changed();
            }
        };
        let _task = match Gds::try_current() {
            Some(gds) => gds.with(|| spawn_local(task_body)),
            None => spawn_local(task_body),
        };
    }

    fn interpret_offer(offer: &ClipboardMimeOffer) -> Result<(Option<Snapshot>, bool), Error> {
        let remote = NativeClipboard::decode_uris(offer)?
            .is_some_and(|(mode, uris)| mode == Mode::Copy && Formats::copyable_uris(&uris));
        match NativeClipboard::decode_snapshot(offer) {
            Ok(snapshot) => Ok((snapshot, remote)),
            Err(Error::InvalidUri(_)) => Ok((None, remote)),
            Err(error) => Err(error),
        }
    }

    /// # Errors
    /// Reports invalid selections, encoding limits, or platform clipboard errors.
    pub async fn write(&self, mode: Mode, paths: Vec<PathBuf>) -> Result<Snapshot, Error> {
        let snapshot = Snapshot::new(mode, paths)?;
        let (written, entries) = NativeClipboard::encode_snapshot(&snapshot)?;
        let seq = self.next_write_seq();
        if let Some(clip) = try_use_clipboard() {
            match clip
                .write_mime(ClipboardKind::Standard, entries, None)
                .await
            {
                Ok(()) => {
                    if self.0.write_seq.get() == seq {
                        self.0.cache.borrow_mut().record_write(&written);
                        self.changed();
                    }
                    return Ok(written);
                }
                Err(error) if should_use_local_fallback(&error) => {
                    let baseline = NativeClipboard::probe_revision(&clip).await;
                    if self.0.write_seq.get() == seq {
                        self.0
                            .cache
                            .borrow_mut()
                            .record_local_write(&written, baseline);
                        self.changed();
                    }
                    return Ok(written);
                }
                Err(error) => return Err(Error::Platform(error)),
            }
        }
        if self.0.write_seq.get() == seq {
            self.0.cache.borrow_mut().record_local_write(&written, None);
            self.changed();
        }
        Ok(written)
    }

    /// # Errors
    /// Reports invalid file or remote URI payloads.
    pub async fn read_payload(&self) -> Result<Payload, Error> {
        if let Some(clip) = try_use_clipboard() {
            match NativeClipboard::read_offer(&clip).await {
                Ok(Some(offer)) => {
                    match NativeClipboard::decode_snapshot(&offer) {
                        Ok(Some(snapshot)) => return Ok(Payload::Local(snapshot)),
                        Ok(None) => {}
                        Err(Error::InvalidUri(_)) => {
                            if let Some((mode, uris)) = NativeClipboard::decode_uris(&offer)? {
                                return Ok(Payload::Uris { mode, uris });
                            }
                            return Ok(Payload::Empty);
                        }
                        Err(error) => return Err(error),
                    }
                    let cache = self.0.cache.borrow();
                    if cache.fallback_matches_platform(Some(offer.revision)) {
                        return Ok(self.fallback_payload(&cache));
                    }
                    return Ok(Payload::Empty);
                }
                Ok(None) => {
                    let cache = self.0.cache.borrow();
                    if cache.fallback_matches_platform(None) {
                        return Ok(self.fallback_payload(&cache));
                    }
                    return Ok(Payload::Empty);
                }
                Err(error) if should_use_local_fallback(&error) => {
                    let cache = self.0.cache.borrow();
                    if cache.is_local_fallback() {
                        return Ok(self.fallback_payload(&cache));
                    }
                    return Ok(Payload::Empty);
                }
                Err(error) => return Err(Error::Platform(error)),
            }
        }
        let cache = self.0.cache.borrow();
        Ok(self.fallback_payload(&cache))
    }

    fn fallback_payload(&self, cache: &Cache) -> Payload {
        if let Some(snapshot) = cache.current() {
            Payload::Local((*snapshot).clone())
        } else if let Some(uris) = cache.local_uris() {
            Payload::Uris {
                mode: Mode::Copy,
                uris,
            }
        } else {
            Payload::Empty
        }
    }

    /// # Errors
    /// Reports invalid remaining paths, encoding failures, or clipboard write errors.
    pub async fn remove(&self, expected: Arc<Snapshot>, paths: Vec<PathBuf>) -> Result<(), Error> {
        let result = self.remove_paths(expected, paths, true).await;
        self.refresh();
        result
    }

    /// # Errors
    /// Reports invalid URI lists or clipboard publication failures.
    pub async fn write_uris(&self, uris: Vec<String>) -> Result<(), Error> {
        let (checked, copyable, entries) = NativeClipboard::encode_uris(&uris)?;
        let seq = self.next_write_seq();
        if let Some(clip) = try_use_clipboard() {
            match clip
                .write_mime(ClipboardKind::Standard, entries, None)
                .await
            {
                Ok(()) => {
                    if self.0.write_seq.get() == seq {
                        self.0.cache.borrow_mut().record_uri_copy(copyable);
                        self.changed();
                    }
                    return Ok(());
                }
                Err(error) if should_use_local_fallback(&error) => {
                    let baseline = NativeClipboard::probe_revision(&clip).await;
                    if self.0.write_seq.get() == seq {
                        self.0
                            .cache
                            .borrow_mut()
                            .record_local_uri_copy(copyable, checked, baseline);
                        self.changed();
                    }
                    return Ok(());
                }
                Err(error) => return Err(Error::Platform(error)),
            }
        }
        if self.0.write_seq.get() == seq {
            self.0
                .cache
                .borrow_mut()
                .record_local_uri_copy(copyable, checked, None);
            self.changed();
        }
        Ok(())
    }

    /// # Errors
    /// Reports clipboard update failures while leaving completed filesystem moves intact.
    pub async fn finish_cut(
        &self,
        expected: Snapshot,
        succeeded: Vec<PathBuf>,
    ) -> Result<(), Error> {
        let snapshot = expected.clone();
        let removed = succeeded.clone();
        let result = self
            .remove_paths(Arc::new(expected), succeeded, false)
            .await;
        if result.is_ok() {
            self.0.cache.borrow_mut().note_pasted(&snapshot, &removed);
            self.changed();
            self.refresh();
        }
        result
    }

    async fn remove_paths(
        &self,
        expected: Arc<Snapshot>,
        removed: Vec<PathBuf>,
        explicit: bool,
    ) -> Result<(), Error> {
        if !explicit && (!expected.can_cancel() || removed.is_empty()) {
            return Ok(());
        }
        let Some(clip) = try_use_clipboard() else {
            self.remove_from_local_cache(&expected, &removed, explicit)?;
            return Ok(());
        };
        match NativeClipboard::read_offer(&clip).await {
            Ok(Some(offer)) => {
                if let Some(current) = NativeClipboard::decode_snapshot(&offer)? {
                    let Some(remaining) = select_remaining(&expected, &current, &removed, explicit)
                    else {
                        return Ok(());
                    };
                    let (next_snapshot, entries) = if remaining.is_empty() {
                        (None, Vec::new())
                    } else {
                        let next = Snapshot::new(expected.mode, remaining)?;
                        let (written, entries) = NativeClipboard::encode_snapshot(&next)?;
                        (Some(written), entries)
                    };
                    let seq = self.next_write_seq();
                    match clip
                        .write_mime(ClipboardKind::Standard, entries, Some(offer.revision))
                        .await
                    {
                        Ok(()) => {
                            if self.0.write_seq.get() == seq {
                                let mut cache = self.0.cache.borrow_mut();
                                match next_snapshot {
                                    Some(written) => cache.record_write(&written),
                                    None => cache.clear(),
                                }
                                drop(cache);
                                self.changed();
                            }
                            return Ok(());
                        }
                        Err(ClipboardError::Changed(_)) => return Ok(()),
                        Err(error) => return Err(Error::Platform(error)),
                    }
                }
                if self
                    .0
                    .cache
                    .borrow()
                    .fallback_matches_platform(Some(offer.revision))
                {
                    self.remove_from_local_cache(&expected, &removed, explicit)?;
                }
                Ok(())
            }
            Ok(None) => {
                if self.0.cache.borrow().fallback_matches_platform(None) {
                    self.remove_from_local_cache(&expected, &removed, explicit)?;
                }
                Ok(())
            }
            Err(error) if should_use_local_fallback(&error) => {
                if self.0.cache.borrow().is_local_fallback() {
                    self.remove_from_local_cache(&expected, &removed, explicit)?;
                }
                Ok(())
            }
            Err(error) => Err(Error::Platform(error)),
        }
    }

    fn remove_from_local_cache(
        &self,
        expected: &Snapshot,
        removed: &[PathBuf],
        explicit: bool,
    ) -> Result<(), Error> {
        let mut cache = self.0.cache.borrow_mut();
        let Some(current) = cache.current() else {
            return Ok(());
        };
        let Some(remaining) = select_remaining(expected, &current, removed, explicit) else {
            return Ok(());
        };
        let baseline = cache.fallback_revision();
        if remaining.is_empty() {
            cache.clear();
        } else {
            let next = Snapshot::new(expected.mode, remaining)?;
            let (written, _) = NativeClipboard::encode_snapshot(&next)?;
            cache.record_local_write(&written, baseline);
        }
        drop(cache);
        self.changed();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
