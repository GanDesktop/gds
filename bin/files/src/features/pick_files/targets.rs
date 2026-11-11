//! Resolve save destinations without replacing an existing item.
use crate::features::file_operations::names;
use std::{
    collections::HashSet,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

const MAX_CANDIDATE_ATTEMPTS: u32 = 10_000;

/// Compute non-colliding save paths in `folder` for each requested leaf in `names`.
///
/// # Errors
/// Propagates filesystem inspection errors other than `NotFound`, or reports
/// `AlreadyExists` when no free numbered candidate is found within the attempt limit.
pub(crate) fn unique(folder: &Path, names: &[OsString]) -> io::Result<Vec<PathBuf>> {
    let mut reserved = HashSet::new();
    let mut targets = Vec::with_capacity(names.len());
    for name in names {
        let initial = folder.join(name);
        if !reserved.contains(&initial) && !occupied(&initial)? {
            reserved.insert(initial.clone());
            targets.push(initial);
            continue;
        }
        let mut chosen = None;
        for index in 1..=MAX_CANDIDATE_ATTEMPTS {
            let candidate = folder.join(names::numbered(name, index));
            if !reserved.contains(&candidate) && !occupied(&candidate)? {
                chosen = Some(candidate);
                break;
            }
        }
        let candidate = chosen.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "no unused save name was found for {} in {}",
                    name.to_string_lossy().escape_debug(),
                    folder.display()
                ),
            )
        })?;
        reserved.insert(candidate.clone());
        targets.push(candidate);
    }
    Ok(targets)
}

/// Check whether `path` already exists on disk without following symbolic links.
///
/// # Errors
/// Propagates filesystem metadata errors other than `NotFound`.
pub(crate) fn occupied(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
