//! Resolve save destinations without replacing an existing item.
use std::{
    collections::HashSet,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

pub(crate) struct SaveTargets;

impl SaveTargets {
    pub(crate) fn unique(folder: &Path, names: &[OsString]) -> io::Result<Vec<PathBuf>> {
        let mut reserved = HashSet::new();
        let mut targets = Vec::with_capacity(names.len());
        for name in names {
            let mut candidate = folder.join(name);
            let mut index = 1;
            while reserved.contains(&candidate) || Self::occupied(&candidate)? {
                let path = Path::new(name);
                let mut name = path.file_stem().unwrap_or(name).to_owned();
                name.push(format!(" ({index})"));
                if let Some(extension) = path.extension() {
                    name.push(".");
                    name.push(extension);
                }
                candidate = folder.join(name);
                index += 1;
            }
            reserved.insert(candidate.clone());
            targets.push(candidate);
        }
        Ok(targets)
    }

    pub(crate) fn occupied(path: &Path) -> io::Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}
