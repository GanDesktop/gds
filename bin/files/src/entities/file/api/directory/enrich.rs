//! Bounded secondary metadata enrichment for directory snapshots.
//!
//! Initial directory reads collect only `lstat` metadata and in-memory filename MIME
//! matches. Child counts, unknown-extension header probes, and symlink target checks
//! run incrementally in bounded batches prioritized by the visible viewport.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::{ChildCount, DirectorySnapshot, Entry, EntryData, EntryKind, Error, LinkTarget};

/// Off-screen entries probed per enrichment pass when unbounded probing is not required.
pub const DEFAULT_PROBE_BUDGET: usize = 256;

/// Maximum viewport-priority entries retained during rapid scrolling before shedding older rows.
pub const DEFAULT_PRIORITY_LIMIT: usize = 128;

/// Default batch flush coalescing window (ms) when secondary metadata cannot alter row order.
pub const DEFAULT_COALESCE_MS: u64 = 32;

/// Default batch flush coalescing window (ms) when secondary metadata can alter order or filter membership.
pub const DEFAULT_REORDER_COALESCE_MS: u64 = 180;

/// Maximum pending enrichment updates buffered before forcing an immediate UI flush.
pub const DEFAULT_COALESCE_MAX_UPDATES: usize = 512;

/// Default worker concurrency per directory enrichment pass.
pub const DEFAULT_MAX_CONCURRENT_BATCHES: usize = 4;

/// Entries processed per blocking batch so UI updates stream progressively and
/// cancellation checks stay responsive.
pub const BATCH_SIZE: usize = 32;
pub const DEFAULT_BATCH_SIZE: usize = BATCH_SIZE;

/// Off-screen probe cap for a background directory enrichment pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeBudget {
    /// Probe at most `n` off-screen entries after satisfying the visible viewport.
    Bounded(usize),
    /// Probe every entry in the directory (required when secondary metadata affects sort or filter).
    Full,
}

impl Default for ProbeBudget {
    fn default() -> Self {
        Self::Bounded(DEFAULT_PROBE_BUDGET)
    }
}

impl ProbeBudget {
    pub const fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }

    pub const fn is_satisfied_by(self, covered: Self) -> bool {
        match (self, covered) {
            (_, Self::Full) => true,
            (Self::Full, Self::Bounded(_)) => false,
            (Self::Bounded(needed), Self::Bounded(done)) => needed <= done,
        }
    }
}

/// Secondary metadata dimensions required by the active view, visible columns,
/// sort key, or file-picker filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnrichmentNeed {
    pub children: bool,
    pub content: bool,
    pub symlinks: bool,
    pub budget: ProbeBudget,
}

impl EnrichmentNeed {
    pub const fn is_empty(self) -> bool {
        !self.children && !self.content && !self.symlinks
    }

    pub const fn is_full_budget(self) -> bool {
        self.budget.is_full()
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
            && self.budget.is_satisfied_by(covered.budget)
    }
}

impl Entry {
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

/// Run secondary filesystem probes for a batch of entries using default child count limits.
///
/// Probe completion flags are recorded even when a probe yields `None` so
/// unreadable directories or files remain `None` without being re-probed on
/// subsequent passes.
///
/// # Errors
/// Returns [`Error::Cancelled`] when `cancel` is triggered before or during the batch.
#[cfg(test)]
pub fn probe_batch(
    entries: &[Entry],
    need: EnrichmentNeed,
    cancel: &CancellationToken,
) -> Result<Vec<Entry>, Error> {
    probe_batch_with_limit(
        entries,
        need,
        super::children::DEFAULT_CHILD_COUNT_LIMIT,
        cancel,
    )
}

/// Run secondary filesystem probes for a batch of entries with a configurable child count bound.
///
/// Probe completion flags are recorded even when a probe yields `None` so
/// unreadable directories or files remain `None` without being re-probed on
/// subsequent passes.
///
/// # Errors
/// Returns [`Error::Cancelled`] when `cancel` is triggered before or during the batch.
pub fn probe_batch_with_limit(
    entries: &[Entry],
    need: EnrichmentNeed,
    child_count_limit: u32,
    cancel: &CancellationToken,
) -> Result<Vec<Entry>, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let child_count_limit = child_count_limit.max(1);
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
                    data.details.children =
                        ChildCount::from_path_with_limit(&data.path, child_count_limit);
                    data.children_probed = true;
                }
            }
            EntryKind::File => {
                if need.content && !data.content_probed {
                    data.details.content = super::content::probe_content(&data.name, &data.path)
                        .or(data.details.content);
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
                    {
                        let target_name = std::fs::read_link(&data.path).ok().and_then(|target| {
                            target.file_name().map(std::ffi::OsStr::to_os_string)
                        });
                        let name = target_name.as_deref().unwrap_or(data.name.as_os_str());
                        *content = super::content::probe_content(name, &data.path)
                            .or_else(|| content.take());
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
