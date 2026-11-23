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
    Task(#[from] gds::service::task_manager::Error),
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

/// Components of a leaf name split into its editable stem and extension(s).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitName<'a> {
    pub stem: &'a OsStr,
    pub compound_mid: Option<&'a OsStr>,
    pub extension: &'a OsStr,
}

/// Split `original` into stem and extension, recognizing common compound `.tar.<ext>` archives
/// (`archive.tar.gz` → stem `archive`, extensions `.tar.gz`) while keeping leading-dot names
/// (`.env`, `.tar.gz`, `..name`) intact as whole stems.
pub fn split_name(original: &OsStr) -> Option<SplitName<'_>> {
    let path = Path::new(original);
    let (stem, extension) = (path.file_stem()?, path.extension()?);
    if stem.is_empty() || stem == OsStr::new(".") || stem == OsStr::new("..") {
        return None;
    }
    if is_tar_compression_ext(extension) {
        if stem.eq_ignore_ascii_case(OsStr::new(".tar")) {
            return None;
        }
        let inner_path = Path::new(stem);
        if let (Some(inner_stem), Some(inner_ext)) =
            (inner_path.file_stem(), inner_path.extension())
            && inner_ext.eq_ignore_ascii_case(OsStr::new("tar"))
            && !inner_stem.is_empty()
            && inner_stem != OsStr::new(".")
            && inner_stem != OsStr::new("..")
        {
            return Some(SplitName {
                stem: inner_stem,
                compound_mid: Some(inner_ext),
                extension,
            });
        }
    }
    Some(SplitName {
        stem,
        compound_mid: None,
        extension,
    })
}

fn is_tar_compression_ext(ext: &OsStr) -> bool {
    let Some(ext) = ext.to_str() else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "gz" | "xz" | "bz2" | "zst" | "lz" | "lzma" | "lz4" | "br" | "lzo" | "z"
    )
}

/// Insert a numbered marker before the extension: `report.pdf` becomes
/// `report (2).pdf`, `archive.tar.gz` becomes `archive (2).tar.gz`, while `.env`
/// and extensionless names keep their whole original text as the stem.
pub fn numbered(original: &OsStr, index: u32) -> OsString {
    let marker = format!(" ({index})");
    match split_name(original) {
        Some(parts) => {
            let mut name = parts.stem.to_os_string();
            name.push(marker);
            if let Some(mid) = parts.compound_mid {
                name.push(".");
                name.push(mid);
            }
            name.push(".");
            name.push(parts.extension);
            name
        }
        None => {
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
            || source.file_name().unwrap_or(source.as_os_str()),
            |target| target.name.as_os_str(),
        )
}

/// # Errors
/// Propagates filesystem errors other than `NotFound` when probing `directory`.
fn is_free(directory: &Path, name: &OsStr, reserved: &HashSet<OsString>) -> Result<bool, Error> {
    Ok(!reserved.contains(name) && !exists(directory.join(name))?)
}

/// # Errors
/// Propagates filesystem inspection errors other than `NotFound`.
fn exists(path: PathBuf) -> Result<bool, Error> {
    match fs::symlink_metadata(&path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(Error::Filesystem { path, source }),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};

    use super::numbered;

    #[test]
    fn numbered_handles_standard_compound_and_dotfile_names() {
        let cases = [
            ("report.pdf", "report (2).pdf"),
            ("docs", "docs (2)"),
            (".env", ".env (2)"),
            ("..name", "..name (2)"),
            ("archive.tar.gz", "archive (2).tar.gz"),
            ("backup.TAR.ZST", "backup (2).TAR.ZST"),
            ("release-v1.2.3.tar.xz", "release-v1.2.3 (2).tar.xz"),
            ("bundle.tar.bz2", "bundle (2).tar.bz2"),
            ("plain.tar", "plain (2).tar"),
            (".tar.gz", ".tar.gz (2)"),
            (".hidden.tar.gz", ".hidden (2).tar.gz"),
            ("notes.final.txt", "notes.final (2).txt"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                numbered(OsStr::new(input), 2),
                OsString::from(expected),
                "unexpected numbered result for {input:?}"
            );
        }
    }
}
