//! Bounded secondary metadata enrichment for directory snapshots.
//!
//! Initial directory reads collect only `lstat` metadata and in-memory filename MIME
//! matches. Child counts, unknown-extension header probes, and symlink target checks
//! run incrementally in bounded batches prioritized by the visible viewport.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::{
    ChildCount, ContentType, DirectorySnapshot, Entry, EntryData, EntryKind, Error, LinkTarget,
};

/// Off-screen entries probed per enrichment pass when `full_budget` is not requested.
pub const DEFAULT_PROBE_BUDGET: usize = 256;

/// Entries processed per blocking batch so UI updates stream progressively and
/// cancellation checks stay responsive.
pub const BATCH_SIZE: usize = 32;

/// Secondary metadata dimensions required by the active view, visible columns,
/// sort key, or file-picker filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnrichmentNeed {
    pub children: bool,
    pub content: bool,
    pub symlinks: bool,
    pub full_budget: bool,
}

impl EnrichmentNeed {
    pub const fn is_empty(self) -> bool {
        !self.children && !self.content && !self.symlinks
    }

    /// Whether a previously completed enrichment pass with `covered` already
    /// satisfies `self`.
    pub const fn is_satisfied_by(self, covered: Self) -> bool {
        if self.is_empty() {
            return true;
        }
        (!self.children || covered.children)
            && (!self.content || covered.content)
            && (!self.symlinks || covered.symlinks)
            && (!self.full_budget || covered.full_budget)
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            children: self.children || other.children,
            content: self.content || other.content,
            symlinks: self.symlinks || other.symlinks,
            full_budget: self.full_budget || other.full_budget,
        }
    }
}

impl Entry {
    /// Whether both handles point to the exact same underlying [`EntryData`] allocation.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Whether this entry still requires secondary I/O to satisfy `need`.
    pub fn needs_enrichment(&self, need: EnrichmentNeed) -> bool {
        match self.kind {
            EntryKind::Directory => need.children && !self.children_probed,
            EntryKind::File => need.content && !self.content_probed,
            EntryKind::SymbolicLink => {
                (!self.target_probed && (need.symlinks || need.content))
                    || (need.content && !self.content_probed)
            }
            EntryKind::Other | EntryKind::Unknown => false,
        }
    }
}

/// Select entries from `snapshot` that still require `need`.
///
/// Entries in `priority_paths` (typically the visible viewport) are scheduled first
/// without a cap; remaining entries in `snapshot.entries` follow in canonical order
/// up to [`DEFAULT_PROBE_BUDGET`] unless `need.full_budget` is set.
#[cfg(test)]
pub fn plan_enrichment(
    snapshot: &DirectorySnapshot,
    priority_paths: &[std::path::PathBuf],
    need: EnrichmentNeed,
) -> Vec<Entry> {
    if need.is_empty() {
        return Vec::new();
    }
    let mut planned = Vec::new();
    let mut seen = std::collections::HashSet::with_capacity(priority_paths.len());
    for path in priority_paths {
        if let Some(entry) = snapshot.entry_for_path(path)
            && entry.needs_enrichment(need)
            && seen.insert(entry.path.as_path())
        {
            planned.push(entry.clone());
        }
    }

    let mut offscreen = 0usize;
    for entry in snapshot.entries.iter() {
        if !need.full_budget && offscreen >= DEFAULT_PROBE_BUDGET {
            break;
        }
        if !entry.needs_enrichment(need) || seen.contains(entry.path.as_path()) {
            continue;
        }
        planned.push(entry.clone());
        offscreen += 1;
    }
    planned
}

/// Run secondary filesystem probes for a batch of entries.
///
/// Probe completion flags are recorded even when a probe yields `None` so
/// unreadable directories or files remain `None` without being re-probed on
/// subsequent passes.
///
/// # Errors
/// Returns [`Error::Cancelled`] when `cancel` is triggered before or during the batch.
pub fn probe_batch(
    entries: &[Entry],
    need: EnrichmentNeed,
    cancel: &CancellationToken,
) -> Result<Vec<Entry>, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let mut updated = Vec::with_capacity(entries.len());
    for entry in entries {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if !entry.needs_enrichment(need) {
            continue;
        }
        let mut data = EntryData::clone(&entry.0);
        match data.kind {
            EntryKind::Directory => {
                if need.children && !data.children_probed {
                    data.details.children = ChildCount::from_path(&data.path);
                    data.children_probed = true;
                }
            }
            EntryKind::File => {
                if need.content && !data.content_probed {
                    if data.details.content.is_none() {
                        data.details.content = ContentType::detect_header(&data.name, &data.path);
                    }
                    data.content_probed = true;
                }
            }
            EntryKind::SymbolicLink => {
                if !data.target_probed && (need.symlinks || need.content) {
                    if need.content {
                        data.details.target = Some(LinkTarget::from_path(&data.path));
                        data.target_probed = true;
                        data.content_probed = true;
                    } else {
                        let (target, content_probed) = LinkTarget::from_path_base(&data.path);
                        data.details.target = Some(target);
                        data.target_probed = true;
                        data.content_probed = content_probed;
                    }
                } else if need.content && !data.content_probed {
                    if let Some(LinkTarget::Resolved {
                        kind: EntryKind::File,
                        content,
                    }) = &mut data.details.target
                        && content.is_none()
                    {
                        *content = ContentType::detect_header(&data.name, &data.path);
                    }
                    data.content_probed = true;
                }
            }
            EntryKind::Other | EntryKind::Unknown => (),
        }
        updated.push(Entry(Arc::new(data)));
    }
    Ok(updated)
}

/// Merge probed `updates` into `snapshot` in place when uniquely owned, or
/// copy-on-write when shared, preserving canonical entry order.
///
/// Returns `Some(changed_visible_metadata)` when at least one entry in `snapshot`
/// was replaced, or `None` when `updates` matched no entries in `snapshot`.
/// Entries whose probe returned `None` still replace their snapshot slot so their
/// `*_probed` state is retained without triggering unnecessary UI re-sorts or repaints.
pub fn merge_enriched_in_place(
    snapshot: &mut Arc<DirectorySnapshot>,
    updates: &[Entry],
) -> Option<bool> {
    if updates.is_empty() {
        return None;
    }
    let current_entries = &snapshot.entries;
    let mut matched: Vec<(usize, &Entry)> = Vec::with_capacity(updates.len());
    let mut changed_visible_metadata = false;

    for update in updates {
        let index = current_entries
            .binary_search_by(|candidate| Entry::canonical_order(candidate, update))
            .ok()
            .or_else(|| {
                current_entries
                    .iter()
                    .position(|candidate| candidate.path == update.path)
            });
        let Some(index) = index else {
            continue;
        };
        if current_entries[index] != *update {
            changed_visible_metadata = true;
        }
        matched.push((index, update));
    }

    if matched.is_empty() {
        return None;
    }

    let snapshot_mut = Arc::make_mut(snapshot);
    if let Some(entries_mut) = Arc::get_mut(&mut snapshot_mut.entries) {
        for (index, update) in matched {
            entries_mut[index] = update.clone();
        }
    } else {
        let mut copied: Vec<Entry> = snapshot_mut.entries.iter().cloned().collect();
        for (index, update) in matched {
            copied[index] = update.clone();
        }
        snapshot_mut.entries = copied.into();
    }

    Some(changed_visible_metadata)
}

/// Merge probed `updates` into `snapshot`, preserving canonical entry order.
///
/// Returns the updated snapshot along with a boolean indicating whether any
/// user-visible metadata field changed (`before != after`). Entries whose probe
/// returned `None` still replace their snapshot slot so their `*_probed` state is
/// retained without triggering unnecessary UI re-sorts or repaints.
#[cfg(test)]
pub fn merge_enriched(
    snapshot: &Arc<DirectorySnapshot>,
    updates: &[Entry],
) -> Option<(Arc<DirectorySnapshot>, bool)> {
    let mut next = Arc::clone(snapshot);
    let changed_visible_metadata = merge_enriched_in_place(&mut next, updates)?;
    Some((next, changed_visible_metadata))
}
