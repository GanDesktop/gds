//! Cancellable sibling directory enumeration using supported image extensions and natural sort.

#[cfg(test)]
mod tests;

use std::{fs, io, os::unix::fs::MetadataExt as _, path::Path};

use gds::service::image::source::Format;
use tokio_util::sync::CancellationToken;

use crate::shared::natural_sort;

use super::model::{CatalogEntry, CatalogEntryMeta, CatalogState};

type FileId = (u64, u64);

struct ScannedEntry {
    id: Option<FileId>,
    is_symlink: bool,
    entry: CatalogEntry,
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

fn normalize_parent_dir(target: &Path) -> Option<&Path> {
    target.parent().map(|parent| {
        if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        }
    })
}

/// Scans `target`'s parent directory for supported sibling images sorted naturally.
///
/// # Errors
/// Returns an I/O error if reading the parent directory fails or if `cancel` is triggered
/// (`io::ErrorKind::Interrupted`).
pub fn scan_siblings(target: &Path, cancel: &CancellationToken) -> io::Result<CatalogState> {
    if cancel.is_cancelled() {
        return Err(cancelled_error());
    }

    let Some(dir) = normalize_parent_dir(target) else {
        let meta = read_file_meta(target);
        return Ok(CatalogState {
            directory: None,
            entries: vec![CatalogEntry::new(target.to_path_buf(), meta)],
            current_index: Some(0),
        });
    };

    let target_id = file_id(target);
    let mut items = scan_directory(dir, cancel)?;

    let current_index = find_target_index(&items, target, target_id).or_else(|| {
        if target.is_file() {
            let (id, meta, is_symlink) = inspect_target(target);
            items.push(ScannedEntry {
                id,
                is_symlink,
                entry: CatalogEntry::new(target.to_path_buf(), meta),
            });
            items.sort_by(|a, b| natural_sort::compare_paths(&a.entry.path, &b.entry.path));
            find_target_index(&items, target, id)
        } else {
            None
        }
    });

    let entries = items.into_iter().map(|item| item.entry).collect();

    Ok(CatalogState {
        directory: Some(dir.to_path_buf()),
        entries,
        current_index,
    })
}

pub fn first_in_folder(
    dir: &Path,
    cancel: &CancellationToken,
) -> io::Result<Option<std::path::PathBuf>> {
    Ok(scan_directory(dir, cancel)?
        .into_iter()
        .next()
        .map(|item| item.entry.path))
}

fn scan_directory(dir: &Path, cancel: &CancellationToken) -> io::Result<Vec<ScannedEntry>> {
    let mut items: Vec<ScannedEntry> = Vec::new();
    for item in fs::read_dir(dir)? {
        if cancel.is_cancelled() {
            return Err(cancelled_error());
        }
        let Ok(entry) = item else {
            continue;
        };
        let path = entry.path();
        if Format::from_path(&path).is_none() {
            continue;
        }
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let is_symlink = ft.is_symlink();
        let (entry_id, meta) = if is_symlink {
            let Ok(m) = fs::metadata(&path) else {
                continue;
            };
            if !m.is_file() {
                continue;
            }
            (
                Some((m.dev(), m.ino())),
                CatalogEntryMeta::new(&path, Some(m.len()), m.modified().ok()),
            )
        } else if ft.is_file() {
            match entry.metadata() {
                Ok(m) => (
                    Some((m.dev(), m.ino())),
                    CatalogEntryMeta::new(&path, Some(m.len()), m.modified().ok()),
                ),
                Err(_) => (None, CatalogEntryMeta::new(&path, None, None)),
            }
        } else {
            continue;
        };
        items.push(ScannedEntry {
            id: entry_id,
            is_symlink,
            entry: CatalogEntry::new(path, meta),
        });
    }

    if cancel.is_cancelled() {
        return Err(cancelled_error());
    }

    items.sort_by(|a, b| natural_sort::compare_paths(&a.entry.path, &b.entry.path));
    Ok(items)
}

fn find_target_index(
    items: &[ScannedEntry],
    target: &Path,
    target_id: Option<FileId>,
) -> Option<usize> {
    // Prefer exact path and lexically normalized component matches before `(dev, ino)` ties,
    // and prefer non-symlink regular files when resolving `(dev, ino)` aliases.
    items
        .iter()
        .position(|item| item.entry.path == target)
        .or_else(|| {
            items
                .iter()
                .position(|item| item.entry.path.components().eq(target.components()))
        })
        .or_else(|| {
            target_id.and_then(|tid| {
                items
                    .iter()
                    .position(|item| !item.is_symlink && item.id == Some(tid))
            })
        })
        .or_else(|| target_id.and_then(|tid| items.iter().position(|item| item.id == Some(tid))))
}

fn file_id(path: &Path) -> Option<FileId> {
    fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

fn inspect_target(path: &Path) -> (Option<FileId>, CatalogEntryMeta, bool) {
    let is_symlink = fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    match fs::metadata(path) {
        Ok(m) => (
            Some((m.dev(), m.ino())),
            CatalogEntryMeta::new(path, Some(m.len()), m.modified().ok()),
            is_symlink,
        ),
        Err(_) => (None, CatalogEntryMeta::new(path, None, None), is_symlink),
    }
}

fn read_file_meta(path: &Path) -> CatalogEntryMeta {
    fs::metadata(path)
        .ok()
        .map(|m| CatalogEntryMeta::new(path, Some(m.len()), m.modified().ok()))
        .unwrap_or_else(|| CatalogEntryMeta::new(path, None, None))
}
