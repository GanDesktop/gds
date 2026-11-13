//! File change sources shared by desktop adapters and application preferences.

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use tokio::sync::mpsc::Sender;

// Resolve the existing prefix even when the watched file has not been created.
// This makes parent-link retargeting visible in the cached target identity too.
fn resolved_target(path: &Path) -> Option<PathBuf> {
    path.ancestors().find_map(|ancestor| {
        let resolved = std::fs::canonicalize(ancestor).ok()?;
        Some(resolved.join(path.strip_prefix(ancestor).ok()?))
    })
}

pub struct Sources {
    requested: Vec<PathBuf>,
    targets: Vec<PathBuf>,
    directories: BTreeSet<PathBuf>,
    sender: Sender<()>,
    watcher: Option<RecommendedWatcher>,
}

impl Sources {
    pub fn new(sender: Sender<()>) -> Self {
        Self {
            requested: Vec::new(),
            targets: Vec::new(),
            directories: BTreeSet::new(),
            sender,
            watcher: None,
        }
    }

    /// Watch parents so atomic replacement, missing files and managed symlinks are followed.
    ///
    /// # Errors
    /// Reports unavailable platform watches; the previous valid watcher is retained.
    pub fn set_paths(&mut self, paths: Vec<PathBuf>) -> notify::Result<()> {
        self.requested = paths;
        self.refresh()
    }

    /// Follow directories and symlink targets that were created or replaced after startup.
    ///
    /// # Errors
    /// Reports unavailable platform watches, retaining the previous watcher.
    pub fn refresh(&mut self) -> notify::Result<()> {
        let targets: BTreeSet<_> = self
            .requested
            .iter()
            .flat_map(|path| std::iter::once(path.clone()).chain(resolved_target(path)))
            .collect();
        let mut essential = BTreeSet::new();
        let mut directories = BTreeSet::new();
        for target in &targets {
            // A watch on a symlinked directory follows its current inode. Also
            // watch the ancestors so replacing any path component wakes refresh.
            let mut parents = target.ancestors().skip(1).filter(|parent| parent.is_dir());
            if let Some(leaf_parent) = parents.next() {
                let leaf_parent = leaf_parent.to_path_buf();
                essential.insert(leaf_parent.clone());
                directories.insert(leaf_parent);
                directories.extend(parents.map(Path::to_path_buf));
            }
        }
        let targets: Vec<_> = targets.into_iter().collect();
        if targets == self.targets && directories == self.directories {
            return Ok(());
        }
        let filter = targets.clone();
        let sender = self.sender.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event)
                    if matches!(
                        event.kind,
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                    ) && event.paths.iter().any(|path| {
                        filter
                            .iter()
                            .any(|target| path == target || target.starts_with(path))
                    }) =>
                {
                    let _ = sender.try_send(());
                }
                Err(error) => tracing::warn!(%error, "desktop file watcher error"),
                _ => {}
            })?;
        let mut active_directories = directories.clone();
        for directory in &directories {
            if let Err(error) = watcher.watch(directory, RecursiveMode::NonRecursive) {
                if matches!(error.kind, notify::ErrorKind::PathNotFound) {
                    tracing::debug!(?directory, %error, "skipping vanished watch directory");
                    active_directories.remove(directory);
                } else if !essential.contains(directory)
                    && !matches!(error.kind, notify::ErrorKind::MaxFilesWatch)
                {
                    tracing::debug!(?directory, %error, "skipping inaccessible watch ancestor");
                } else {
                    return Err(error);
                }
            }
        }
        self.targets = targets;
        self.directories = active_directories;
        self.watcher = Some(watcher);
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
