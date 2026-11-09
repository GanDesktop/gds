//! Dolphin-compatible user tags. The filesystem is authoritative; no index lives here.
use std::{io, path::Path};

pub const ATTRIBUTE: &str = "user.xdg.tags";
#[cfg(target_os = "linux")]
const MAX_ATTRIBUTE_SIZE: usize = 65536;

/// Decode KDE's comma-separated tag format without CSV escaping.
///
/// # Errors
/// Returns an error if the attribute is not valid UTF-8.
pub fn decode(bytes: &[u8]) -> io::Result<Vec<String>> {
    let text =
        std::str::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(text
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}

/// # Errors
/// Rejects empty tags and characters that cannot be stored or edited losslessly.
pub fn validate(tag: &str) -> io::Result<()> {
    if tag.is_empty() || tag.contains([',', '\0', '\n', '\r']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "A tag must be nonempty and contain no comma, NUL or newline",
        ));
    }
    Ok(())
}

/// Read the target's tags, following symlinks like KFileMetaData on Linux.
///
/// # Errors
/// Returns filesystem, unsupported-platform or invalid UTF-8 errors.
pub fn read(path: &Path) -> io::Result<Vec<String>> {
    read_raw(path)?.map_or_else(|| Ok(Vec::new()), |bytes| decode(&bytes))
}

#[cfg(target_os = "linux")]
fn read_raw(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = vec![0; MAX_ATTRIBUTE_SIZE];
    match rustix::fs::getxattr(path, ATTRIBUTE, bytes.as_mut_slice()) {
        Ok(count) => {
            bytes.truncate(count);
            Ok(Some(bytes))
        }
        Err(rustix::io::Errno::NODATA) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(not(target_os = "linux"))]
fn read_raw(_: &Path) -> io::Result<Option<Vec<u8>>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "File tags require Linux",
    ))
}

/// Replace the target's tags, removing the attribute when empty.
///
/// # Errors
/// Returns invalid-tag, filesystem or unsupported-platform errors.
pub fn write(path: &Path, tags: &[String]) -> io::Result<()> {
    for tag in tags {
        validate(tag)?;
    }
    let mut unique: Vec<&str> = Vec::new();
    for tag in tags {
        if !unique.contains(&tag.as_str()) {
            unique.push(tag);
        }
    }
    write_raw(path, unique.join(",").as_bytes())
}

#[cfg(target_os = "linux")]
fn write_raw(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() {
        match rustix::fs::removexattr(path, ATTRIBUTE) {
            Ok(()) | Err(rustix::io::Errno::NODATA) => Ok(()),
            Err(e) => Err(e.into()),
        }
    } else {
        rustix::fs::setxattr(path, ATTRIBUTE, bytes, rustix::fs::XattrFlags::empty())
            .map_err(Into::into)
    }
}

#[cfg(not(target_os = "linux"))]
fn write_raw(_: &Path, _: &[u8]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "File tags require Linux",
    ))
}

/// Change only the requested association, preserving all other tags.
///
/// # Errors
/// Returns errors from tag validation, reading or writing the attribute.
pub fn change(path: &Path, tag: &str, add: bool) -> io::Result<()> {
    validate(tag)?;
    let mut tags = read(path)?;
    if add {
        if !tags.iter().any(|s| s == tag) {
            tags.push(tag.to_owned());
        }
    } else {
        tags.retain(|s| s != tag);
    }
    write(path, &tags)
}

/// Apply only the editor's additions/removals, retaining unrelated external edits.
/// xattrs have no compare-and-swap; concurrent writers still require coordination.
///
/// # Errors
/// Rejects symbolic links and special files; propagates validation and attribute I/O errors.
pub fn apply_edit(path: &Path, original: &[String], edited: &[String]) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Open the symbolic link target to edit tags",
        ));
    }
    for tag in edited {
        validate(tag)?;
    }
    let mut current = read(path)?;
    current.retain(|tag| !original.contains(tag) || edited.contains(tag));
    for tag in edited {
        if !original.contains(tag) && !current.contains(tag) {
            current.push(tag.clone());
        }
    }
    write(path, &current)
}

/// Copy from the same descriptor whose contents the transfer has validated.
///
/// # Errors
/// Returns attribute I/O errors other than an absent or unsupported source attribute.
#[cfg(target_os = "linux")]
pub fn copy_from(source: &std::fs::File, destination: &Path) -> io::Result<()> {
    let mut bytes = vec![0; MAX_ATTRIBUTE_SIZE];
    match rustix::fs::fgetxattr(source, ATTRIBUTE, bytes.as_mut_slice()) {
        Ok(count) => write_raw(destination, &bytes[..count]),
        Err(rustix::io::Errno::NODATA | rustix::io::Errno::OPNOTSUPP) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Preserve raw bytes during a copy; files with no xattr support have no tags to copy.
///
/// # Errors
/// Returns errors reading or writing attributes, except unsupported source attributes.
pub fn copy(source: &Path, destination: &Path) -> io::Result<()> {
    match read_raw(source) {
        Ok(Some(bytes)) => write_raw(destination, &bytes),
        Ok(None) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::Unsupported => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests;
