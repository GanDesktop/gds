//! Leaf names for transfers and folder creation: validation, conflict
//! detection and free-name suggestions.
//!
//! Names stay [`OsString`]s: source paths are never rebuilt from a lossy
//! display string, and a suggested name derived from a non-UTF-8 leaf keeps
//! its original bytes.

use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

/// How many numbered candidates are tried before giving up on a name.
const MAX_NAME_ATTEMPTS: u32 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("the name is empty")]
    Empty,
    #[error("the name contains a path separator")]
    Separator,
    #[error("the name is a reserved directory name")]
    Reserved,
    #[error("the name contains an invalid character")]
    Invalid,
}

impl NameError {
    /// Message id describing this rejection in the file manager's dialogs.
    pub fn label_id(self) -> &'static str {
        match self {
            Self::Empty => "transfer-name-empty",
            Self::Separator => "transfer-name-separator",
            Self::Reserved => "transfer-name-reserved",
            Self::Invalid => "transfer-name-invalid",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Name(#[from] NameError),
    #[error("cannot inspect {path}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("no unused name was found for {name} in {directory}")]
    Exhausted { directory: PathBuf, name: String },
}

/// A source transferred as `name` instead of its original leaf.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedTarget {
    pub source: PathBuf,
    pub name: OsString,
}

impl NamedTarget {
    pub fn new(source: PathBuf, name: OsString) -> Self {
        Self { source, name }
    }
}

/// A source whose planned target is already taken in the destination folder or
/// collides with another source's target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collision {
    pub source: PathBuf,
    pub target: OsString,
}

impl Collision {
    /// Display form of the conflicting target, safe for dialog text.
    pub fn target_label(&self) -> String {
        self.target.to_string_lossy().escape_debug().to_string()
    }
}

/// Check one leaf name; no filesystem access is performed.
///
/// The comparison is byte-exact: leading and trailing spaces are legal names
/// and are never trimmed implicitly.
///
/// # Errors
/// Rejects empty names, path separators, NUL bytes and the reserved names `.`
/// and `..`.
pub fn validate(name: &OsStr) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    let bytes = name.as_encoded_bytes();
    if bytes.contains(&b'/') || (cfg!(windows) && bytes.contains(&b'\\')) {
        return Err(NameError::Separator);
    }
    if bytes.contains(&0) {
        return Err(NameError::Invalid);
    }
    if name == OsStr::new(".") || name == OsStr::new("..") {
        return Err(NameError::Reserved);
    }
    Ok(())
}

/// Insert a numbered marker before the extension: `report.pdf` becomes
/// `report (2).pdf`, while `.env` and extensionless names keep their whole
/// original text as the stem.
pub fn numbered(original: &OsStr, index: u32) -> OsString {
    let marker = format!(" ({index})");
    let path = Path::new(original);
    match (path.file_stem(), path.extension()) {
        // A leading-dot name keeps its whole text as the stem: `.env` and the
        // dot-only stems `Path` reports for `..name` must not gain a fake tail.
        (Some(stem), Some(extension))
            if !stem.is_empty() && stem != OsStr::new(".") && stem != OsStr::new("..") =>
        {
            let mut name = stem.to_os_string();
            name.push(marker);
            name.push(".");
            name.push(extension);
            name
        }
        _ => {
            let mut name = original.to_os_string();
            name.push(marker);
            name
        }
    }
}

/// Pick a name in `directory` free both on disk and from `reserved`.
///
/// The original name is preferred; otherwise ` (2)`, ` (3)`, … are appended.
///
/// # Errors
/// Rejects an invalid original name and reports directories whose entries
/// cannot be inspected.
pub fn free_name(
    directory: &Path,
    original: &OsStr,
    reserved: &HashSet<OsString>,
) -> Result<OsString, Error> {
    validate(original)?;
    if is_free(directory, original, reserved)? {
        return Ok(original.to_owned());
    }
    for index in 2..=MAX_NAME_ATTEMPTS {
        let candidate = numbered(original, index);
        if is_free(directory, &candidate, reserved)? {
            return Ok(candidate);
        }
    }
    Err(Error::Exhausted {
        directory: directory.to_owned(),
        name: original.to_string_lossy().escape_debug().to_string(),
    })
}

/// The leaf a source will be transferred as, honoring an explicit choice.
pub fn target_name<'a>(source: &'a Path, chosen: &'a [NamedTarget]) -> &'a OsStr {
    chosen
        .iter()
        .find(|target| target.source == source)
        .map_or_else(
            || source.file_name().unwrap_or_else(|| source.as_os_str()),
            |target| target.name.as_os_str(),
        )
}

/// Planned target names that already exist in `destination` or are shared by
/// more than one source, in source order.
///
/// Duplicates are reported for every source after the first one, so renaming
/// them in order never transfers two entries onto the same leaf.
///
/// # Errors
/// Reports entries of `destination` that cannot be inspected; an unreadable
/// target is never treated as free.
pub fn collisions(
    destination: &Path,
    sources: &[PathBuf],
    chosen: &[NamedTarget],
) -> Result<Vec<Collision>, Error> {
    let mut planned = HashSet::new();
    let mut collisions = Vec::new();
    for source in sources {
        let target = target_name(source, chosen).to_owned();
        let duplicate = !planned.insert(target.clone());
        if duplicate || exists(destination.join(&target))? {
            collisions.push(Collision {
                source: source.clone(),
                target,
            });
        }
    }
    Ok(collisions)
}

fn is_free(directory: &Path, name: &OsStr, reserved: &HashSet<OsString>) -> Result<bool, Error> {
    Ok(!reserved.contains(name) && !exists(directory.join(name))?)
}

fn exists(path: PathBuf) -> Result<bool, Error> {
    match fs::symlink_metadata(&path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(Error::Filesystem { path, source }),
    }
}

#[cfg(test)]
mod tests;
