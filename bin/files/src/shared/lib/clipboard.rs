//! File clipboard interchange; Wayland uses real file MIME offers.
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
use serde::{Deserialize, Serialize};

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
    #[error("file clipboard changed while its formats were being read")]
    Changed,
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
    NativeRead(#[from] gds::service::desktop::clipboard::Error),
    #[cfg(target_os = "linux")]
    #[error("Wayland clipboard write failed: {0}")]
    WaylandWrite(#[from] wl_clipboard_rs::copy::Error),
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
        let checked = parse_uri_list(&uris.join("\n"));
        match self.backend {
            #[cfg(target_os = "linux")]
            Backend::Wayland => cx
                .background_executor()
                .spawn(async move { wayland::Clipboard::write_uris(checked?) }),
            Backend::Gpui => {
                if let Ok(uris) = &checked {
                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string_with_metadata(
                        uris.join("\n"),
                        REMOTE_URI_METADATA.into(),
                    ));
                }
                Task::ready(checked.map(|_| ()))
            }
        }
    }

    pub fn read_uris(&self, cx: &mut App) -> Task<Result<Option<(Mode, Vec<String>)>, Error>> {
        match self.backend {
            #[cfg(target_os = "linux")]
            Backend::Wayland => cx
                .background_executor()
                .spawn(async { wayland::Clipboard::read_uris() }),
            Backend::Gpui => Task::ready(fallback::Clipboard::decode_uris(
                cx.read_from_clipboard().as_ref(),
            )),
        }
    }
    pub fn new(_: &App) -> Self {
        #[cfg(target_os = "linux")]
        if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
            || std::env::var_os("WAYLAND_SOCKET").is_some_and(|value| !value.is_empty())
        {
            return Self {
                backend: Backend::Wayland,
            };
        }
        Self {
            backend: Backend::Gpui,
        }
    }

    /// Start on the GPUI thread. Wayland connection and payload work run in the background.
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
            Backend::Wayland => cx.background_executor().spawn(async move {
                let snapshot = Snapshot::new(mode, paths)?;
                wayland::Clipboard::write(&snapshot)
            }),
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
            Backend::Wayland => cx
                .background_executor()
                .spawn(async { wayland::Clipboard::read() }),
            Backend::Gpui => fallback::Clipboard::read(cx),
        }
    }

    /// Remove completed cut entries only from an unchanged GDS-owned snapshot.
    /// Foreign owners and newly copied contents are left untouched. The Wayland
    /// protocol exposes no atomic compare-and-set; ownership is reread immediately
    /// before publishing a replacement, rather than assuming the old offer persists.
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
            Backend::Wayland => cx
                .background_executor()
                .spawn(async move { wayland::Clipboard::finish_cut(&snapshot, &paths) }),
            Backend::Gpui => fallback::Clipboard::finish_cut(snapshot, paths, cx),
        }
    }
}

fn parse_uri_list(text: &str) -> Result<Vec<String>, Error> {
    if text.len() > protocol::MAX_PAYLOAD_BYTES {
        return Err(Error::TooLarge);
    }
    let uris = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let uri = url::Url::parse(line).map_err(|e| Error::InvalidPayload(e.to_string()))?;
            if uri.password().is_some() {
                return Err(Error::InvalidPayload(
                    "passwords are not allowed in file URIs".into(),
                ));
            }
            Ok(uri.to_string())
        })
        .collect::<Result<Vec<_>, Error>>()?;
    if uris.is_empty() {
        return Err(Error::InvalidPayload("empty URI list".into()));
    }
    Ok(uris)
}

/// Whether `path` is one of the sources of the current in-application cut.
///
/// This reads only the cached intent; no clipboard or filesystem I/O happens
/// during rendering. The cache changes only when a clipboard operation
/// completes or [`refresh`] validates it.
pub fn is_cut(path: &Path, cx: &App) -> bool {
    cx.try_global::<state::Cache>()
        .is_some_and(|cache| cache.is_cut(path))
}

/// Last known file contents, shared by all windows without clipboard I/O in render.
pub fn current(cx: &App) -> Option<Arc<Snapshot>> {
    cx.try_global::<state::Cache>()
        .and_then(state::Cache::current)
}

/// Re-read file contents on activation or returning from a file interaction.
///
/// Callers invoke this on window activation or on a returning file-row click;
/// it never polls. One read is in flight at a time, and a read that started
/// before a newer copy/cut cannot replace that newer intent.
pub fn refresh(cx: &mut App) {
    let Some(generation) = cx.default_global::<state::Cache>().begin_refresh() else {
        return;
    };
    let bridge = Bridge::new(cx);
    let read = bridge.read(cx);
    cx.spawn(async move |cx| {
        let current = read.await;
        let changed = cx.update(|cx| {
            cx.default_global::<state::Cache>()
                .reconcile(generation, current)
        });
        if changed {
            cx.update(|cx| cx.refresh_windows());
        }
    })
    .detach();
}

/// Forget file contents when a known text or other payload replaces them.
pub fn clear(cx: &mut App) {
    let changed = {
        let cache = cx.default_global::<state::Cache>();
        let had_files = cache.current().is_some();
        cache.clear();
        had_files
    };
    if changed {
        cx.refresh_windows();
    }
}

/// Cancel an owned cut, clearing the platform offer only while it still matches.
/// A later clipboard write is preserved. Repeated Escape presses share the
/// pending cancellation; errors leave the cut visible and available to retry.
///
/// # Errors
/// The task reports clipboard access or update failures.
pub fn cancel_cut(cx: &mut App) -> Option<Task<Result<(), Error>>> {
    current(cx).filter(|snapshot| snapshot.can_cancel())?;
    let Some(expected) = cx.default_global::<state::Cache>().begin_cancel() else {
        return Some(Task::ready(Ok(())));
    };
    let cleanup =
        Bridge::new(cx).remove_cut_paths(expected.as_ref().clone(), expected.paths.clone(), cx);
    Some(cx.spawn(async move |cx| {
        let result = cleanup.await;
        cx.update(|cx| {
            cx.default_global::<state::Cache>()
                .finish_cancel(&expected, result.is_ok());
            refresh(cx);
            cx.refresh_windows();
        });
        result
    }))
}

/// Record a clipboard write only after it actually succeeded.
pub(crate) fn note_clipboard_write(written: &Snapshot, cx: &mut App) {
    cx.default_global::<state::Cache>().record_write(written);
    cx.refresh_windows();
}

/// Drop the sources a cut paste moved, keeping any entry it did not move.
pub(crate) fn note_cut_paste(pasted: &Snapshot, succeeded: &[PathBuf], cx: &mut App) {
    if pasted.mode != Mode::Cut || succeeded.is_empty() {
        return;
    }
    cx.default_global::<state::Cache>()
        .note_pasted(pasted, succeeded);
    cx.refresh_windows();
}

#[cfg(test)]
mod tests;
