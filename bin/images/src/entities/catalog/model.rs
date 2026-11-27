//! Ordered sibling list, cached metadata labels, and O(log N) cursor helpers.

#[cfg(test)]
mod tests;

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use gds::service::image::source::Format;
use std::sync::Arc;

use crate::shared::natural_sort;

/// Formats a byte count using compact binary KiB/MiB units.
pub fn format_byte_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogEntryMeta {
    pub bytes: Option<u64>,
    pub modified: Option<SystemTime>,
    pub display_name: Arc<str>,
    pub subtitle: Arc<str>,
}

impl CatalogEntryMeta {
    /// Builds entry metadata with pre-formatted filmstrip title and `"Format · Size"` subtitle.
    pub fn new(path: &Path, bytes: Option<u64>, modified: Option<SystemTime>) -> Self {
        let display_name: Arc<str> = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        let subtitle: Arc<str> = format_subtitle(path, bytes).into();
        Self {
            bytes,
            modified,
            display_name,
            subtitle,
        }
    }
}

fn format_subtitle(path: &Path, bytes: Option<u64>) -> String {
    let format_label = Format::from_path(path)
        .map(|fmt| fmt.label().to_owned())
        .unwrap_or_else(|| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_uppercase())
                .unwrap_or_default()
        });
    let size_label = bytes.map(format_byte_size);
    match (format_label.is_empty(), size_label) {
        (false, Some(size)) => format!("{format_label} · {size}"),
        (false, None) => format_label,
        (true, Some(size)) => size,
        (true, None) => String::new(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    pub path: PathBuf,
    pub meta: CatalogEntryMeta,
}

impl CatalogEntry {
    pub fn new(path: PathBuf, meta: CatalogEntryMeta) -> Self {
        Self { path, meta }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogState {
    pub directory: Option<PathBuf>,
    pub entries: Vec<CatalogEntry>,
    pub current_index: Option<usize>,
}

impl CatalogState {
    pub fn position_display(&self) -> Option<(usize, usize)> {
        let index = self.current_index?;
        (!self.entries.is_empty()).then_some((index + 1, self.entries.len()))
    }

    pub fn entry_at(&self, index: usize) -> Option<&CatalogEntry> {
        self.entries.get(index)
    }

    pub fn meta_at(&self, index: usize) -> Option<&CatalogEntryMeta> {
        self.entries.get(index).map(|entry| &entry.meta)
    }

    pub fn meta_for_path(&self, target: &Path) -> Option<&CatalogEntryMeta> {
        let idx = self.find_index(target)?;
        self.meta_at(idx)
    }

    pub fn current_path(&self) -> Option<&PathBuf> {
        let idx = self.current_index?;
        self.entry_at(idx).map(|entry| &entry.path)
    }

    pub fn select_path(&mut self, target: &Path) -> bool {
        if let Some(idx) = self.find_index(target) {
            self.current_index = Some(idx);
            true
        } else {
            false
        }
    }

    pub fn can_step_prev(&self) -> bool {
        self.entries.len() > 1
    }

    pub fn can_step_next(&self) -> bool {
        self.entries.len() > 1
    }

    pub fn peek_prev(&self) -> Option<PathBuf> {
        let current = self.current_index?;
        if self.entries.len() <= 1 {
            return None;
        }
        let prev = if current == 0 {
            self.entries.len() - 1
        } else {
            current - 1
        };
        self.entries.get(prev).map(|entry| entry.path.clone())
    }

    pub fn peek_next(&self) -> Option<PathBuf> {
        let current = self.current_index?;
        if self.entries.len() <= 1 {
            return None;
        }
        let next = (current + 1) % self.entries.len();
        self.entries.get(next).map(|entry| entry.path.clone())
    }

    pub fn first(&self) -> Option<PathBuf> {
        self.entries.first().map(|entry| entry.path.clone())
    }

    pub fn last(&self) -> Option<PathBuf> {
        self.entries.last().map(|entry| entry.path.clone())
    }

    /// Returns a contiguous slice starting at `offset` clamped around `current_index` without padding empty slots.
    pub fn visible_window(&self, capacity: usize) -> (usize, &[CatalogEntry]) {
        let len = self.entries.len();
        if len == 0 || capacity == 0 {
            return (0, &[]);
        }
        let show = len.min(capacity);
        let cur = self.current_index.filter(|&i| i < len).unwrap_or(0);
        let half = show / 2;
        let max_start = len - show;
        let start = cur.saturating_sub(half).min(max_start);
        (start, &self.entries[start..start + show])
    }

    pub fn rename_path(&mut self, old: &Path, new_path: PathBuf, new_meta: CatalogEntryMeta) {
        let Some(pos) = self.find_index(old) else {
            return;
        };
        self.entries.remove(pos);
        let insert_idx = self
            .entries
            .binary_search_by(|entry| natural_sort::compare_paths(&entry.path, &new_path))
            .unwrap_or_else(|idx| idx);
        self.entries
            .insert(insert_idx, CatalogEntry::new(new_path, new_meta));
        self.current_index = if self.current_index == Some(pos) {
            Some(insert_idx)
        } else {
            self.current_index.map(|mut cur| {
                if cur > pos {
                    cur -= 1;
                }
                if insert_idx <= cur {
                    cur += 1;
                }
                cur
            })
        };
    }

    /// Synchronously updates an entry's cached size, mtime, and formatted subtitle after save/commit.
    pub fn update_entry_meta(
        &mut self,
        path: &Path,
        byte_len: u64,
        modified: Option<SystemTime>,
    ) -> bool {
        let Some(pos) = self.find_index(path) else {
            return false;
        };
        let entry = &mut self.entries[pos];
        let next_modified = modified.or(entry.meta.modified);
        entry.meta = CatalogEntryMeta::new(&entry.path, Some(byte_len), next_modified);
        true
    }

    pub fn remove_path(&mut self, removed: &Path) -> Option<PathBuf> {
        let pos = self.find_index(removed)?;
        self.entries.remove(pos);
        if self.entries.is_empty() {
            self.current_index = None;
            return None;
        }
        let next_idx = pos.min(self.entries.len() - 1);
        match self.current_index {
            Some(cur) if cur == pos => self.current_index = Some(next_idx),
            Some(cur) if cur > pos => self.current_index = Some(cur - 1),
            Some(_) | None => {}
        }
        self.entries.get(next_idx).map(|entry| entry.path.clone())
    }

    fn find_index(&self, target: &Path) -> Option<usize> {
        self.entries
            .binary_search_by(|entry| natural_sort::compare_paths(&entry.path, target))
            .ok()
    }
}
