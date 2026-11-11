//! File clipboard interchange through the application window's native selection.
//! Other platforms retain a clearly limited GPUI same-process fallback.

mod fallback;
mod protocol;
mod state;
#[cfg(target_os = "linux")]
mod wayland;

use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

use gpui_kit::{App, Task};
use protocol::Formats;
use serde::{Deserialize, Serialize};
use state::Cache;

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
        let remaining: Vec<_> = self
            .paths
            .iter()
            .filter(|path| !removed.contains(path))
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
        let completed: HashSet<_> = succeeded.iter().collect();
        let remaining: Vec<_> = self
            .paths
            .iter()
            .filter(|path| !completed.contains(path))
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
    #[error("platform clipboard read failed: {0}")]
    PlatformRead(#[from] gpui_kit::ClipboardReadError),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    Native(#[from] gpui_kit::ClipboardAccessError),
}

#[derive(Clone, Copy, Debug)]
enum Backend {
    #[cfg(target_os = "linux")]
    Wayland,
    Gpui,
}

#[derive(Clone, Copy, Debug)]
pub struct Bridge {
    backend: Backend,
}

const REMOTE_URI_METADATA: &str = "gds-remote-uris";

impl Bridge {
    /// # Errors
    /// Reports invalid URI lists or an unavailable clipboard provider.
    pub fn write_uris(&self, uris: Vec<String>, cx: &mut App) -> Task<Result<(), Error>> {
        let checked = Formats::parse_uri_list(&uris.join("\n"));
        let copyable = checked
            .as_ref()
            .is_ok_and(|uris| Formats::copyable_uris(uris));
        let write = match self.backend {
            #[cfg(target_os = "linux")]
            Backend::Wayland => {
                Task::ready(checked.and_then(|uris| wayland::Clipboard::write_uris(uris, cx)))
            }
            Backend::Gpui => {
                if let Ok(uris) = &checked {
                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string_with_metadata(
                        uris.join("\n"),
                        REMOTE_URI_METADATA.into(),
                    ));
                }
                Task::ready(checked.map(|_| ()))
            }
        };
        cx.spawn(async move |cx| {
            write.await?;
            cx.update(|cx| {
                cx.default_global::<Cache>().record_uri_copy(copyable);
                cx.refresh_windows();
            });
            Ok(())
        })
    }

    /// # Errors
    /// The task reports malformed URI lists or clipboard access failures.
    pub fn read_uris(&self, cx: &mut App) -> Task<Result<Option<(Mode, Vec<String>)>, Error>> {
        match self.backend {
            #[cfg(target_os = "linux")]
            Backend::Wayland => wayland::Clipboard::read_uris(cx),
            Backend::Gpui => Task::ready(fallback::Clipboard::decode_uris(
                cx.read_from_clipboard().as_ref(),
            )),
        }
    }

    pub fn new(cx: &App) -> Self {
        #[cfg(target_os = "linux")]
        if cx.supports_clipboard_mime() {
            return Self {
                backend: Backend::Wayland,
            };
        }
        Self {
            backend: Backend::Gpui,
        }
    }

    /// Publish from the GPUI thread using the focused window's selection serial.
    ///
    /// # Errors
    /// The task reports invalid paths, encoding or unavailable clipboard protocols.
    pub fn write(
        &self,
        mode: Mode,
        paths: Vec<PathBuf>,
        cx: &mut App,
    ) -> Task<Result<Snapshot, Error>> {
        match self.backend {
            #[cfg(target_os = "linux")]
            Backend::Wayland => Task::ready(
                Snapshot::new(mode, paths)
                    .and_then(|snapshot| wayland::Clipboard::write(&snapshot, cx)),
            ),
            Backend::Gpui => Task::ready(fallback::Clipboard::write(mode, paths, cx)),
        }
    }

    /// Read supported file clipboard formats, never infer files from ordinary text.
    ///
    /// # Errors
    /// The task reports malformed, non-local or inaccessible file clipboard data.
    pub fn read(&self, cx: &mut App) -> Task<Result<Option<Snapshot>, Error>> {
        match self.backend {
            #[cfg(target_os = "linux")]
            Backend::Wayland => wayland::Clipboard::read(cx),
            Backend::Gpui => fallback::Clipboard::read(cx),
        }
    }

    /// Remove completed cut entries only from an unchanged GDS-owned snapshot.
    /// Foreign owners and newly copied contents are left untouched. Wayland cleanup
    /// compares the captured offer revision immediately before publication.
    ///
    /// # Errors
    /// The task reports clipboard access or update failures; filesystem results are unaffected.
    pub fn finish_cut(
        &self,
        snapshot: Snapshot,
        succeeded: Vec<PathBuf>,
        cx: &mut App,
    ) -> Task<Result<(), Error>> {
        self.remove_cut_paths(snapshot, succeeded, cx)
    }

    /// Remove an explicitly chosen entry, only while the platform offer matches the displayed snapshot.
    ///
    /// # Errors
    /// Reports clipboard read or publication failures.
    pub fn remove_path(
        &self,
        snapshot: Snapshot,
        path: PathBuf,
        cx: &mut App,
    ) -> Task<Result<(), Error>> {
        match self.backend {
            #[cfg(target_os = "linux")]
            Backend::Wayland => wayland::Clipboard::remove_paths(snapshot, vec![path], true, cx),
            Backend::Gpui => fallback::Clipboard::remove_paths(snapshot, vec![path], true, cx),
        }
    }

    /// # Errors
    /// The task reports clipboard read or publication failures.
    fn remove_cut_paths(
        &self,
        snapshot: Snapshot,
        paths: Vec<PathBuf>,
        cx: &mut App,
    ) -> Task<Result<(), Error>> {
        if !snapshot.can_cancel() || paths.is_empty() {
            return Task::ready(Ok(()));
        }
        match self.backend {
            #[cfg(target_os = "linux")]
            Backend::Wayland => wayland::Clipboard::remove_paths(snapshot, paths, false, cx),
            Backend::Gpui => fallback::Clipboard::finish_cut(snapshot, paths, cx),
        }
    }
}

/// Whether `path` is one of the sources of the current in-application cut.
///
/// This reads only the cached intent; no clipboard or filesystem I/O happens
/// during rendering. The cache changes only when a clipboard operation
/// completes or [`refresh`] validates it.
pub fn is_cut(path: &Path, cx: &App) -> bool {
    Cache::is_cut_in(path, cx)
}

/// Last known file contents, shared by all windows without clipboard I/O in render.
pub fn current(cx: &App) -> Option<Arc<Snapshot>> {
    Cache::current_in(cx)
}

/// Whether the last validated offer contains local files or copyable remote URIs.
pub fn can_paste(cx: &App) -> bool {
    Cache::can_paste_in(cx)
}

/// Re-read file contents on activation or returning from a file interaction.
///
/// Callers invoke this on window activation or on a returning file-row click;
/// it never polls. One read is in flight at a time, and a read that started
/// before a newer copy/cut cannot replace that newer intent.
pub fn refresh(cx: &mut App) {
    Cache::refresh(cx);
}

/// Forget file contents when a known text or other payload replaces them.
pub fn clear(cx: &mut App) {
    Cache::clear_in(cx);
}

/// Cancel an owned cut, clearing the platform offer only while it still matches.
/// A later clipboard write is preserved. Repeated Escape presses share the
/// pending cancellation; errors leave the cut visible and available to retry.
///
/// # Errors
/// The task reports clipboard access or update failures.
pub fn cancel_cut(cx: &mut App) -> Option<Task<Result<(), Error>>> {
    Cache::cancel_cut(cx)
}

/// Record a clipboard write only after it actually succeeded.
pub(crate) fn note_clipboard_write(written: &Snapshot, cx: &mut App) {
    Cache::note_write(written, cx);
}

/// Drop the sources a cut paste moved, keeping any entry it did not move.
pub(crate) fn note_cut_paste(pasted: &Snapshot, succeeded: &[PathBuf], cx: &mut App) {
    Cache::note_cut_paste(pasted, succeeded, cx);
}

#[cfg(test)]
mod tests;
