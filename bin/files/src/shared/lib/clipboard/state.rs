//! Cached file clipboard contents for commands, the footer, and cut appearance.

use std::{collections::HashSet, path::Path, path::PathBuf, sync::Arc};

use gpui_kit::{App, BorrowAppContext as _, Global, Task};

use super::{Bridge, Error, Mode, Snapshot, protocol::Formats};

#[derive(Default)]
pub(super) struct Cache {
    snapshot: Option<Arc<Snapshot>>,
    remote_copy: bool,
    cut_paths: HashSet<PathBuf>,
    generation: u64,
    refreshing: bool,
    cancelling: bool,
}

impl Global for Cache {}

impl Cache {
    pub(super) fn observe<V: 'static>(
        cx: &mut gpui_kit::Context<V>,
        on_change: impl Fn(&mut V, &mut gpui_kit::Context<V>) + 'static,
    ) -> gpui_kit::Subscription {
        cx.observe_global::<Self>(on_change)
    }

    fn mutate<R>(cx: &mut App, f: impl FnOnce(&mut Self) -> R) -> R {
        cx.update_default_global::<Self, R>(|cache, _| f(cache))
    }

    fn notify_observers(cx: &mut App) {
        Self::mutate(cx, |_| ());
    }

    pub(super) fn is_cut_in(path: &Path, cx: &App) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|cache| cache.is_cut(path))
    }

    pub(super) fn current_in(cx: &App) -> Option<Arc<Snapshot>> {
        cx.try_global::<Self>().and_then(Self::current)
    }

    pub(super) fn can_paste_in(cx: &App) -> bool {
        cx.try_global::<Self>().is_some_and(Self::can_paste)
    }

    pub(super) fn refresh(cx: &mut App) {
        let Some(generation) = cx.default_global::<Self>().begin_refresh() else {
            return;
        };
        let bridge = Bridge::new(cx);
        let read = bridge.read(cx);
        cx.spawn(async move |cx| {
            let current = read.await;
            let (current, remote_copy) = match current {
                Ok(None) | Err(Error::InvalidUri(_)) => {
                    match cx.update(|cx| bridge.read_uris(cx)).await {
                        Ok(offer) => {
                            let copyable = offer.is_some_and(|(mode, uris)| {
                                mode == Mode::Copy && Formats::copyable_uris(&uris)
                            });
                            (Ok(None), copyable)
                        }
                        Err(error) => (Err(error), false),
                    }
                }
                result => (result, false),
            };
            cx.update(|cx| {
                if cx
                    .default_global::<Self>()
                    .reconcile(generation, current, remote_copy)
                {
                    Self::notify_observers(cx);
                    cx.refresh_windows();
                }
            });
        })
        .detach();
    }

    pub(super) fn clear_in(cx: &mut App) {
        let changed = {
            let cache = cx.default_global::<Self>();
            let had_files = cache.can_paste();
            cache.clear();
            had_files
        };
        if changed {
            Self::notify_observers(cx);
            cx.refresh_windows();
        }
    }

    /// # Errors
    /// The task reports clipboard access or update failures.
    pub(super) fn cancel_cut(cx: &mut App) -> Option<Task<Result<(), Error>>> {
        Self::current_in(cx).filter(|snapshot| snapshot.can_cancel())?;
        let Some(expected) = cx.default_global::<Self>().begin_cancel() else {
            return Some(Task::ready(Ok(())));
        };
        let cleanup =
            Bridge::new(cx).remove_cut_paths(expected.as_ref().clone(), expected.paths.clone(), cx);
        Some(cx.spawn(async move |cx| {
            let result = cleanup.await;
            cx.update(|cx| {
                cx.default_global::<Self>()
                    .finish_cancel(&expected, result.is_ok());
                if result.is_ok() {
                    Self::notify_observers(cx);
                }
                Self::refresh(cx);
                cx.refresh_windows();
            });
            result
        }))
    }

    pub(super) fn note_write(written: &Snapshot, cx: &mut App) {
        Self::mutate(cx, |cache| cache.record_write(written));
        cx.refresh_windows();
    }

    pub(super) fn record_uri_copy_in(copyable: bool, cx: &mut App) {
        Self::mutate(cx, |cache| cache.record_uri_copy(copyable));
        cx.refresh_windows();
    }

    pub(super) fn note_cut_paste(pasted: &Snapshot, succeeded: &[PathBuf], cx: &mut App) {
        if pasted.mode != Mode::Cut || succeeded.is_empty() {
            return;
        }
        Self::mutate(cx, |cache| cache.note_pasted(pasted, succeeded));
        cx.refresh_windows();
    }

    pub(super) fn current(&self) -> Option<Arc<Snapshot>> {
        self.snapshot.clone()
    }

    pub(super) fn can_paste(&self) -> bool {
        self.snapshot.is_some() || self.remote_copy
    }

    pub(super) fn is_cut(&self, path: &Path) -> bool {
        self.cut_paths.contains(path)
    }

    pub(super) fn has_cut(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.mode == Mode::Cut)
    }

    pub(super) fn record_write(&mut self, written: &Snapshot) {
        self.replace(Some(written.clone()));
    }

    pub(super) fn record_uri_copy(&mut self, copyable: bool) {
        self.replace(None);
        self.remote_copy = copyable;
    }

    pub(super) fn clear(&mut self) {
        self.replace(None);
    }

    fn replace(&mut self, snapshot: Option<Snapshot>) {
        self.remote_copy = false;
        self.generation = self.generation.wrapping_add(1);
        self.refreshing = false;
        self.cancelling = false;
        self.cut_paths = snapshot
            .as_ref()
            .filter(|snapshot| snapshot.mode == Mode::Cut)
            .map(|snapshot| snapshot.paths.iter().cloned().collect())
            .unwrap_or_default();
        self.snapshot = snapshot.map(Arc::new);
    }

    pub(super) fn note_pasted(&mut self, pasted: &Snapshot, succeeded: &[PathBuf]) {
        let Some(current) = self
            .snapshot
            .as_ref()
            .filter(|current| current.mode == Mode::Cut && current.identity == pasted.identity)
        else {
            return;
        };
        let completed: HashSet<_> = succeeded.iter().collect();
        let mut remaining = current.as_ref().clone();
        remaining.paths.retain(|path| !completed.contains(path));
        if remaining.paths.len() != current.paths.len() {
            self.replace((!remaining.paths.is_empty()).then_some(remaining));
        }
    }

    pub(super) fn begin_refresh(&mut self) -> Option<u64> {
        if self.refreshing || self.cancelling {
            return None;
        }
        self.refreshing = true;
        Some(self.generation)
    }

    /// Older reads cannot replace a newer copy, cut, or cancellation. Read
    /// errors retain the last known contents instead of pretending it is empty.
    pub(super) fn reconcile(
        &mut self,
        generation: u64,
        current: Result<Option<Snapshot>, Error>,
        remote_copy: bool,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.refreshing = false;
        let Ok(current) = current else {
            return false;
        };
        if self.snapshot.as_deref() == current.as_ref() && self.remote_copy == remote_copy {
            return false;
        }
        self.replace(current);
        self.remote_copy = remote_copy;
        true
    }

    pub(super) fn begin_cancel(&mut self) -> Option<Arc<Snapshot>> {
        if !self.has_cut() || self.cancelling {
            return None;
        }
        let snapshot = self.current().filter(|snapshot| snapshot.can_cancel())?;
        self.generation = self.generation.wrapping_add(1);
        self.refreshing = false;
        self.cancelling = true;
        Some(snapshot)
    }

    pub(super) fn finish_cancel(&mut self, expected: &Snapshot, success: bool) {
        if self.snapshot.as_deref() != Some(expected) || !self.cancelling {
            return;
        }
        if success {
            self.clear();
        } else {
            self.cancelling = false;
        }
    }
}
