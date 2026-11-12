use super::super as directory;
use notify::EventKind;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

const MAX_PATHS: usize = 256;

#[derive(Default)]
pub(crate) struct Changes {
    paths: HashSet<PathBuf>,
    full: bool,
}

impl Changes {
    pub fn rescan(&mut self) {
        self.full = true;
        self.paths.clear();
    }

    pub fn push(&mut self, event: notify::Event) {
        if event.need_rescan()
            || event.paths.is_empty()
            || matches!(event.kind, EventKind::Any | EventKind::Other)
        {
            self.rescan();
        }
        if self.full {
            return;
        }
        self.paths.extend(event.paths);
        if self.paths.len() > MAX_PATHS {
            self.rescan();
        }
    }

    /// Apply queued filesystem notifications to an owned directory snapshot.
    ///
    /// # Errors
    /// Reports directory open or read failures when a full rescan is required.
    #[cfg(test)]
    pub fn apply(
        self,
        snapshot: directory::DirectorySnapshot,
    ) -> Result<directory::DirectorySnapshot, directory::Error> {
        let shared = Arc::new(snapshot);
        let cancel = CancellationToken::new();
        match self.apply_shared(&shared, &cancel)? {
            Some(updated) => Ok(Arc::unwrap_or_clone(updated)),
            None => Ok(Arc::unwrap_or_clone(shared)),
        }
    }

    /// Reconcile queued filesystem notifications against a shared snapshot without
    /// deep-cloning untouched entries.
    ///
    /// Returns `Ok(None)` when neither entries nor warning state changed.
    ///
    /// # Errors
    /// Returns [`directory::Error::Cancelled`] when `cancel` is triggered, or
    /// directory open/read failures when a full rescan is required.
    pub fn apply_shared(
        self,
        snapshot: &Arc<directory::DirectorySnapshot>,
        cancel: &CancellationToken,
    ) -> Result<Option<Arc<directory::DirectorySnapshot>>, directory::Error> {
        if cancel.is_cancelled() {
            return Err(directory::Error::Cancelled);
        }
        if self.full || self.paths.contains(&snapshot.path) {
            return Self::rescan_shared(snapshot, cancel);
        }
        if self.paths.is_empty() {
            return Ok(None);
        }

        let index_by_path: HashMap<&Path, usize> = snapshot
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.path.as_path(), index))
            .collect();
        let mut entries: Vec<Option<directory::Entry>> =
            snapshot.entries.iter().cloned().map(Some).collect();
        let mut warnings = snapshot.warnings.clone();
        let mut identities = directory::IdentityCache::new();
        let mut touched = false;

        for path in self.paths {
            if cancel.is_cancelled() {
                return Err(directory::Error::Cancelled);
            }
            if path.parent() != Some(snapshot.path.as_path()) {
                continue;
            }
            let before_warnings = warnings.len();
            warnings.retain(|warning| warning.path != path);
            if warnings.len() != before_warnings {
                touched = true;
            }
            if !snapshot.options.show_hidden
                && path
                    .file_name()
                    .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
            {
                if let Some(&index) = index_by_path.get(path.as_path())
                    && entries[index].take().is_some()
                {
                    touched = true;
                }
                continue;
            }
            match directory::Entry::inspect_base_with_identities(&path, &mut identities) {
                Ok(entry) => {
                    if let Some(&index) = index_by_path.get(path.as_path()) {
                        let entry = match entries[index].as_ref() {
                            Some(previous) => entry.with_reused_probes(previous),
                            None => entry,
                        };
                        if entries[index].as_ref() != Some(&entry) {
                            touched = true;
                        }
                        entries[index] = Some(entry);
                    } else {
                        entries.push(Some(entry));
                        touched = true;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if let Some(&index) = index_by_path.get(path.as_path())
                        && entries[index].take().is_some()
                    {
                        touched = true;
                    }
                }
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "cannot update watched entry; rescanning");
                    return Self::rescan_shared(snapshot, cancel);
                }
            }
        }

        if !touched {
            return Ok(None);
        }

        let mut next_entries: Vec<directory::Entry> = entries.into_iter().flatten().collect();
        next_entries.sort_by(directory::Entry::canonical_order);
        if next_entries.as_slice() == snapshot.entries.as_ref()
            && warnings.len() == snapshot.warnings.len()
        {
            return Ok(None);
        }

        Ok(Some(Arc::new(directory::DirectorySnapshot {
            path: snapshot.path.clone(),
            entries: next_entries.into(),
            warnings,
            options: snapshot.options,
        })))
    }

    fn rescan_shared(
        snapshot: &Arc<directory::DirectorySnapshot>,
        cancel: &CancellationToken,
    ) -> Result<Option<Arc<directory::DirectorySnapshot>>, directory::Error> {
        let mut next = directory::read_directory_base(&snapshot.path, snapshot.options, cancel)?;
        if let Some(entries) = Arc::get_mut(&mut next.entries) {
            reuse_unchanged_probes(entries, &snapshot.entries);
        }
        if next.entries == snapshot.entries
            && next.warnings.is_empty() == snapshot.warnings.is_empty()
        {
            return Ok(None);
        }
        Ok(Some(Arc::new(next)))
    }
}

fn reuse_unchanged_probes(entries: &mut [directory::Entry], previous: &[directory::Entry]) {
    let mut prev_index = 0;
    for entry in entries.iter_mut() {
        while let Some(prev) = previous.get(prev_index) {
            match directory::Entry::canonical_order(prev, entry) {
                std::cmp::Ordering::Less => prev_index += 1,
                std::cmp::Ordering::Equal => {
                    entry.reuse_probes(prev);
                    prev_index += 1;
                    break;
                }
                std::cmp::Ordering::Greater => break,
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
