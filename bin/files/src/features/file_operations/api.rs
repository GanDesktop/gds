//! File operations on an immutable selection of original filesystem paths.

pub mod names;
pub mod transfer;

use std::{
    collections::HashSet,
    fs, io,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Trash,
    PermanentDelete,
}

impl Kind {
    /// # Errors
    /// Propagates inspection, parent resolution and the selected operation's failure.
    fn execute(self, path: &Path) -> Result<(), Error> {
        match self {
            Self::Trash => trash::delete(path).map_err(Error::from),
            Self::PermanentDelete => Self::delete_permanently(path),
        }
    }

    /// # Errors
    /// Reports inaccessible or missing entries and failures to remove the entry.
    fn delete_permanently(path: &Path) -> Result<(), Error> {
        let metadata = fs::symlink_metadata(path).map_err(|source| Error::Filesystem {
            path: path.to_owned(),
            source,
        })?;
        let parent = path.parent().ok_or_else(|| Error::InvalidTarget {
            path: path.to_owned(),
        })?;
        let name = path.file_name().ok_or_else(|| Error::InvalidTarget {
            path: path.to_owned(),
        })?;
        let target = parent
            .canonicalize()
            .map_err(|source| Error::Filesystem {
                path: path.to_owned(),
                source,
            })?
            .join(name);
        // Only the parent is resolved. remove_dir_all also avoids following links
        // encountered within the directory; a link leaf always uses remove_file.
        let result = if metadata.file_type().is_dir() {
            fs::remove_dir_all(&target)
        } else {
            fs::remove_file(&target)
        };
        result.map_err(|source| Error::Filesystem {
            path: path.to_owned(),
            source,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no files are selected")]
    EmptySelection,
    #[error("refusing to operate on a path without an absolute, named entry: {path}")]
    InvalidTarget { path: PathBuf },
    #[error("path cannot be copied as lossless UTF-8 text: {path}")]
    NonUtf8Clipboard { path: PathBuf },
    #[error("filesystem operation failed for {path}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Trash(#[from] trash::Error),
}

#[derive(Clone, Debug)]
pub struct Batch {
    paths: Vec<PathBuf>,
}

impl Batch {
    /// Freeze unique absolute entry paths without resolving their leaf components.
    /// Parent traversal is not lexically collapsed: it may cross a symlink.
    ///
    /// # Errors
    /// Rejects an empty selection, relative paths and roots or parent-directory targets.
    pub fn new(paths: Vec<PathBuf>) -> Result<Self, Error> {
        let mut seen = HashSet::new();
        let mut unique = Vec::with_capacity(paths.len());
        for path in paths {
            if !path.is_absolute()
                || !matches!(path.components().next_back(), Some(Component::Normal(_)))
            {
                return Err(Error::InvalidTarget { path });
            }
            if seen.insert(path.clone()) {
                unique.push(path);
            }
        }
        if unique.is_empty() {
            return Err(Error::EmptySelection);
        }
        Ok(Self { paths: unique })
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Build clipboard text atomically; no selected path is converted lossily.
    ///
    /// # Errors
    /// Reports the first path that cannot be represented in UTF-8.
    pub fn clipboard_text(&self) -> Result<String, Error> {
        self.paths
            .iter()
            .map(|path| {
                path.to_str()
                    .ok_or_else(|| Error::NonUtf8Clipboard { path: path.clone() })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|paths| paths.join("\n"))
    }

    /// Execute the chosen operation on a background executor, retaining every outcome.
    /// A failed operation never falls back to another kind of deletion.
    pub fn execute(self, kind: Kind) -> Report {
        self.execute_with(kind, |path| kind.execute(path))
    }

    fn execute_with(
        self,
        kind: Kind,
        mut operation: impl FnMut(&Path) -> Result<(), Error>,
    ) -> Report {
        let items = self
            .paths
            .into_iter()
            .map(|path| {
                let result = operation(&path);
                ItemResult { path, result }
            })
            .collect();
        Report { kind, items }
    }
}

#[derive(Debug)]
pub struct ItemResult {
    pub path: PathBuf,
    pub result: Result<(), Error>,
}

#[derive(Debug)]
pub struct Report {
    pub kind: Kind,
    pub items: Vec<ItemResult>,
}

impl Report {
    pub fn succeeded_paths(&self) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter(|item| item.result.is_ok())
            .map(|item| item.path.clone())
            .collect()
    }

    pub fn failed_items(&self) -> impl Iterator<Item = &ItemResult> {
        self.items.iter().filter(|item| item.result.is_err())
    }
}

#[cfg(test)]
mod tests;
