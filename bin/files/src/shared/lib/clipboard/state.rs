//! Cached file clipboard contents shared by the footer and entry appearance.

use std::{collections::HashSet, path::Path, path::PathBuf, sync::Arc};

use gpui_kit::Global;

use super::{Error, Mode, Snapshot};

#[derive(Default)]
pub(super) struct Cache {
    snapshot: Option<Arc<Snapshot>>,
    cut_paths: HashSet<PathBuf>,
    generation: u64,
    refreshing: bool,
    cancelling: bool,
}

impl Global for Cache {}

impl Cache {
    pub(super) fn current(&self) -> Option<Arc<Snapshot>> {
        self.snapshot.clone()
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

    pub(super) fn clear(&mut self) {
        self.replace(None);
    }

    fn replace(&mut self, snapshot: Option<Snapshot>) {
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
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.refreshing = false;
        let Ok(current) = current else {
            return false;
        };
        if self.snapshot.as_deref() == current.as_ref() {
            return false;
        }
        self.replace(current);
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
