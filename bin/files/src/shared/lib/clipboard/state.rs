//! Cached file clipboard contents for commands, the footer, cut appearance, and unfocused fallback.

use std::{collections::HashSet, path::Path, path::PathBuf, sync::Arc};

use super::{Error, Mode, Snapshot};

#[derive(Default)]
pub(super) struct Cache {
    snapshot: Option<Arc<Snapshot>>,
    remote_copy: bool,
    portal_transfer: bool,
    cut_paths: HashSet<PathBuf>,
    generation: u64,
    refreshing: bool,
    local_fallback: bool,
    local_uris: Option<Vec<String>>,
    fallback_revision: Option<u64>,
}

impl Cache {
    pub(super) fn current(&self) -> Option<Arc<Snapshot>> {
        self.snapshot.clone()
    }

    pub(super) fn can_paste(&self) -> bool {
        self.snapshot.is_some() || self.remote_copy || self.portal_transfer
    }

    pub(super) fn is_cut(&self, path: &Path) -> bool {
        self.cut_paths.contains(path)
    }

    pub(super) fn is_local_fallback(&self) -> bool {
        self.local_fallback
    }

    pub(super) fn fallback_revision(&self) -> Option<u64> {
        self.fallback_revision
    }

    pub(super) fn fallback_matches_platform(&self, observed_revision: Option<u64>) -> bool {
        self.local_fallback && self.fallback_revision == observed_revision
    }

    pub(super) fn local_uris(&self) -> Option<Vec<String>> {
        self.local_uris.clone()
    }

    pub(super) fn record_write(&mut self, written: &Snapshot) {
        self.replace(Some(written.clone()));
    }

    pub(super) fn record_uri_copy(&mut self, copyable: bool) {
        self.replace(None);
        self.remote_copy = copyable;
    }

    pub(super) fn record_local_write(
        &mut self,
        written: &Snapshot,
        baseline_revision: Option<u64>,
    ) {
        self.replace(Some(written.clone()));
        self.local_fallback = true;
        self.fallback_revision = baseline_revision;
    }

    pub(super) fn record_local_uri_copy(
        &mut self,
        copyable: bool,
        uris: Vec<String>,
        baseline_revision: Option<u64>,
    ) {
        self.replace(None);
        self.remote_copy = copyable;
        self.local_fallback = true;
        self.local_uris = Some(uris);
        self.fallback_revision = baseline_revision;
    }

    pub(super) fn clear(&mut self) {
        self.replace(None);
    }

    fn replace(&mut self, snapshot: Option<Snapshot>) {
        self.remote_copy = false;
        self.portal_transfer = false;
        self.local_fallback = false;
        self.local_uris = None;
        self.fallback_revision = None;
        self.generation = self.generation.wrapping_add(1);
        self.refreshing = false;
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
        let was_local = self.local_fallback;
        let baseline = self.fallback_revision;
        let completed: HashSet<_> = succeeded.iter().collect();
        let mut remaining = current.as_ref().clone();
        remaining.paths.retain(|path| !completed.contains(path));
        if remaining.paths.len() != current.paths.len() {
            if remaining.paths.is_empty() {
                self.clear();
            } else if was_local {
                self.record_local_write(&remaining, baseline);
            } else {
                self.replace(Some(remaining));
            }
        }
    }

    pub(super) fn begin_refresh(&mut self) -> Option<u64> {
        if self.refreshing {
            return None;
        }
        self.refreshing = true;
        Some(self.generation)
    }

    pub(super) fn finish_refresh_unchanged(&mut self, generation: u64) {
        if generation == self.generation {
            self.refreshing = false;
        }
    }

    /// Older reads cannot replace a newer copy, cut, or cancellation. Read
    /// errors retain the last known contents instead of pretending it is empty.
    pub(super) fn reconcile(
        &mut self,
        generation: u64,
        current: Result<Option<Snapshot>, Error>,
        remote_copy: bool,
        portal_transfer: bool,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.refreshing = false;
        let Ok(current) = current else {
            return false;
        };
        if self.snapshot.as_deref() == current.as_ref()
            && self.remote_copy == remote_copy
            && self.portal_transfer == portal_transfer
            && !self.local_fallback
        {
            return false;
        }
        self.replace(current);
        self.remote_copy = remote_copy;
        self.portal_transfer = portal_transfer;
        true
    }
}
