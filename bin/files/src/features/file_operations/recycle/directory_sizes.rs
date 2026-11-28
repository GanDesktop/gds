//! FreeDesktop Trash Specification 1.0 `directorysizes` cache support.
//!
//! Each trash root (`$trash`, parent of `info/` and `files/`) may contain a
//! `directorysizes` file with lines formatted as:
//! `<size_in_bytes> <mtime_epoch_seconds> <percent_encoded_directory_name>`
//! where `<mtime_epoch_seconds>` is the whole-second modification time of
//! `$trash/info/<name>.trashinfo`.
//!
//! Reference: <https://specifications.freedesktop.org/trash-spec/1.0/>

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs, io,
    os::unix::ffi::OsStringExt as _,
    path::{Path, PathBuf},
    sync::Mutex,
};

use percent_encoding::percent_decode;

use crate::features::file_operations::names;

pub(super) const FILE_NAME: &str = "directorysizes";
const MAX_CACHE_BYTES: u64 = 16 * 1024 * 1024;

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// One parsed `directorysizes` cache entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SizeRecord {
    pub(super) size: u64,
    pub(super) mtime: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParsedLine {
    name: OsString,
    record: SizeRecord,
}

/// Per-root in-memory cache so listing trash loads each `$trash/directorysizes`
/// file at most once per unique trash root encountered.
#[derive(Default)]
pub(super) struct DirectorySizesCache {
    roots: HashMap<PathBuf, HashMap<OsString, SizeRecord>>,
}

impl DirectorySizesCache {
    pub(super) fn lookup(
        &mut self,
        trash_root: &Path,
        stem: &OsStr,
        info_mtime: i64,
    ) -> Option<u64> {
        let map = self
            .roots
            .entry(trash_root.to_path_buf())
            .or_insert_with(|| load_root(trash_root));
        let record = map.get(stem)?;
        (record.mtime == info_mtime).then_some(record.size)
    }

    #[cfg(test)]
    pub(super) fn loaded_root_count(&self) -> usize {
        self.roots.len()
    }
}

/// Parse raw `directorysizes` bytes, ignoring malformed lines.
///
/// Duplicate entries for the same decoded directory name resolve in favor of
/// the last valid line in the file.
pub(super) fn parse(bytes: &[u8]) -> HashMap<OsString, SizeRecord> {
    let mut entries = HashMap::new();
    for line in bytes.split(|&byte| byte == b'\n') {
        if let Some(parsed) = parse_line(line) {
            entries.insert(parsed.name, parsed.record);
        }
    }
    entries
}

/// Atomically remove all entries matching `stem` from `$trash/directorysizes`,
/// preserving all other lines verbatim.
///
/// Missing `directorysizes` files or files without a matching entry succeed
/// without touching disk.
///
/// # Errors
/// Reports I/O errors reading, creating a sibling temp file, or renaming over
/// `directorysizes`, or if `directorysizes` is not a regular file / exceeds
/// the safety size limit.
pub(super) fn remove_entry(trash_root: &Path, stem: &OsStr) -> io::Result<()> {
    let _guard = WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cache_path = trash_root.join(FILE_NAME);
    let metadata = match fs::symlink_metadata(&cache_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "directorysizes is not a regular file",
        ));
    }
    if metadata.len() > MAX_CACHE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "directorysizes exceeds size limit",
        ));
    }
    let existing = fs::read(&cache_path)?;
    let Some(updated) = filter_out_entry(&existing, stem) else {
        return Ok(());
    };
    atomic_write(
        &cache_path,
        trash_root,
        &updated,
        Some(metadata.permissions()),
    )
}

fn load_root(trash_root: &Path) -> HashMap<OsString, SizeRecord> {
    let path = trash_root.join(FILE_NAME);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return HashMap::new(),
        Err(error) => {
            tracing::debug!(
                path = %path.display(),
                error = %error,
                "Unable to inspect trash directorysizes cache"
            );
            return HashMap::new();
        }
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_CACHE_BYTES {
        tracing::debug!(
            path = %path.display(),
            size = metadata.len(),
            "Skipping unusable trash directorysizes cache"
        );
        return HashMap::new();
    }
    match fs::read(&path) {
        Ok(bytes) => parse(&bytes),
        Err(error) => {
            tracing::debug!(
                path = %path.display(),
                error = %error,
                "Unable to read trash directorysizes cache"
            );
            HashMap::new()
        }
    }
}

fn parse_line(raw: &[u8]) -> Option<ParsedLine> {
    let line = raw.strip_suffix(b"\n").unwrap_or(raw);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.is_empty() {
        return None;
    }
    let (size_bytes, after_size) = split_once_byte(line, b' ')?;
    let (mtime_bytes, encoded_name) = split_once_byte(after_size, b' ')?;

    if size_bytes.is_empty() || !size_bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let size = std::str::from_utf8(size_bytes).ok()?.parse::<u64>().ok()?;

    let mtime_digits = mtime_bytes.strip_prefix(b"-").unwrap_or(mtime_bytes);
    if mtime_digits.is_empty() || !mtime_digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mtime = std::str::from_utf8(mtime_bytes).ok()?.parse::<i64>().ok()?;

    if encoded_name.is_empty() || encoded_name.contains(&b' ') {
        return None;
    }
    let decoded: Vec<u8> = percent_decode(encoded_name).collect();
    let name = OsString::from_vec(decoded);
    names::validate(&name).ok()?;

    Some(ParsedLine {
        name,
        record: SizeRecord { size, mtime },
    })
}

fn split_once_byte(input: &[u8], needle: u8) -> Option<(&[u8], &[u8])> {
    let index = input.iter().position(|&byte| byte == needle)?;
    let (head, tail) = input.split_at(index);
    let (_, rest) = tail.split_first()?;
    Some((head, rest))
}

fn filter_out_entry(bytes: &[u8], stem: &OsStr) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut removed = false;
    for raw_line in bytes.split_inclusive(|&byte| byte == b'\n') {
        if parse_line(raw_line).is_some_and(|parsed| parsed.name.as_os_str() == stem) {
            removed = true;
        } else {
            output.extend_from_slice(raw_line);
        }
    }
    removed.then_some(output)
}

fn atomic_write(
    target: &Path,
    trash_root: &Path,
    content: &[u8],
    permissions: Option<fs::Permissions>,
) -> io::Result<()> {
    use std::io::Write as _;

    let mut temp = tempfile::Builder::new()
        .prefix(".directorysizes.")
        .tempfile_in(trash_root)?;
    if let Some(perms) = permissions {
        let _ = temp.as_file().set_permissions(perms);
    }
    temp.write_all(content)?;
    temp.as_file().sync_data()?;
    temp.persist(target).map_err(|error| error.error)?;
    Ok(())
}
