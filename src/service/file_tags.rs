//! Dolphin-compatible user tags and portable `user.*` FreeDesktop extended attributes.
//! The filesystem is authoritative; no index lives here.
use std::{io, path::Path};

pub const ATTRIBUTE: &str = "user.xdg.tags";
pub const COMMENT_ATTRIBUTE: &str = "user.xdg.comment";
pub const ORIGIN_URL_ATTRIBUTE: &str = "user.xdg.origin.url";
pub const MIME_TYPE_ATTRIBUTE: &str = "user.mime_type";
/// Maximum byte length allowed for `user.xdg.comment`.
pub const MAX_COMMENT_BYTES: usize = 4096;
#[cfg(target_os = "linux")]
const MAX_ATTRIBUTE_SIZE: usize = 65536;
#[cfg(target_os = "linux")]
const INITIAL_XATTR_LIST_SIZE: usize = 4096;
#[cfg(target_os = "linux")]
const MAX_XATTR_LIST_SIZE: usize = 65536;

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

fn encode(tags: &[String]) -> io::Result<Vec<u8>> {
    for tag in tags {
        validate(tag)?;
    }
    let mut unique: Vec<&str> = Vec::new();
    for tag in tags {
        if !unique.contains(&tag.as_str()) {
            unique.push(tag);
        }
    }
    Ok(unique.join(",").into_bytes())
}

/// Replace the target's tags, removing the attribute when empty.
///
/// # Errors
/// Returns invalid-tag, filesystem or unsupported-platform errors.
pub fn write(path: &Path, tags: &[String]) -> io::Result<()> {
    write_raw(path, &encode(tags)?)
}

#[cfg(target_os = "linux")]
fn open_target(path: &Path) -> io::Result<std::os::fd::OwnedFd> {
    use rustix::{
        fs::{CWD, FileType, Mode, OFlags, fstat, openat},
        io::Errno,
    };

    // O_NONBLOCK prevents opening a FIFO from blocking before fstat rejects special files.
    let fd = match openat(
        CWD,
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::LOOP) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Open the symbolic link target to edit tags",
            ));
        }
        Err(error) => return Err(error.into()),
    };
    let stat = fstat(&fd)?;
    if !matches!(
        FileType::from_raw_mode(stat.st_mode),
        FileType::RegularFile | FileType::Directory
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Open the symbolic link target to edit tags",
        ));
    }
    Ok(fd)
}

#[cfg(target_os = "linux")]
fn read_raw_fd<Fd: rustix::fd::AsFd>(fd: Fd) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = vec![0; MAX_ATTRIBUTE_SIZE];
    match rustix::fs::fgetxattr(fd, ATTRIBUTE, bytes.as_mut_slice()) {
        Ok(count) => {
            bytes.truncate(count);
            Ok(Some(bytes))
        }
        Err(rustix::io::Errno::NODATA) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(target_os = "linux")]
fn write_raw_fd<Fd: rustix::fd::AsFd>(fd: Fd, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() {
        match rustix::fs::fremovexattr(fd, ATTRIBUTE) {
            Ok(()) | Err(rustix::io::Errno::NODATA) => Ok(()),
            Err(e) => Err(e.into()),
        }
    } else {
        rustix::fs::fsetxattr(fd, ATTRIBUTE, bytes, rustix::fs::XattrFlags::empty())
            .map_err(Into::into)
    }
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
#[cfg(target_os = "linux")]
pub fn apply_edit(path: &Path, original: &[String], edited: &[String]) -> io::Result<()> {
    let fd = open_target(path)?;
    for tag in edited {
        validate(tag)?;
    }
    let mut current = read_raw_fd(&fd)?.map_or_else(|| Ok(Vec::new()), |bytes| decode(&bytes))?;
    current.retain(|tag| !original.contains(tag) || edited.contains(tag));
    for tag in edited {
        if !original.contains(tag) && !current.contains(tag) {
            current.push(tag.clone());
        }
    }
    write_raw_fd(&fd, &encode(&current)?)
}

/// Apply only the editor's additions/removals, retaining unrelated external edits.
/// xattrs have no compare-and-swap; concurrent writers still require coordination.
///
/// # Errors
/// Rejects symbolic links and special files; propagates validation and attribute I/O errors.
#[cfg(not(target_os = "linux"))]
pub fn apply_edit(_: &Path, _: &[String], _: &[String]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "File tags require Linux",
    ))
}

/// Read a file's FreeDesktop user comment (`user.xdg.comment`), returning `Ok(None)` when absent
/// or when the underlying filesystem does not support extended attributes.
///
/// # Errors
/// Returns filesystem errors (other than `ENODATA` / `EOPNOTSUPP` / `ENOSYS`) or invalid UTF-8.
pub fn read_comment(path: &Path) -> io::Result<Option<String>> {
    #[cfg(target_os = "linux")]
    {
        let mut bytes = vec![0; MAX_ATTRIBUTE_SIZE];
        match rustix::fs::getxattr(path, COMMENT_ATTRIBUTE, bytes.as_mut_slice()) {
            Ok(count) => {
                bytes.truncate(count);
                let text = std::str::from_utf8(&bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                if text.trim().is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(text.to_owned()))
                }
            }
            Err(
                rustix::io::Errno::NODATA | rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::NOSYS,
            ) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Ok(None)
    }
}

/// Write or clear a file's FreeDesktop user comment (`user.xdg.comment`).
///
/// Passing `None` or whitespace-only text removes the attribute.
///
/// # Errors
/// Rejects NUL bytes, overly long comments (> [`MAX_COMMENT_BYTES`]), symbolic links and special
/// files; propagates extended-attribute I/O failures.
pub fn write_comment(path: &Path, comment: Option<&str>) -> io::Result<()> {
    let value = comment.map(str::trim).filter(|text| !text.is_empty());
    if let Some(text) = value {
        if text.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Comment cannot contain NUL bytes",
            ));
        }
        if text.len() > MAX_COMMENT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Comment exceeds maximum length of {MAX_COMMENT_BYTES} bytes"),
            ));
        }
    }
    #[cfg(target_os = "linux")]
    {
        let fd = open_target(path)?;
        match value {
            None => match rustix::fs::fremovexattr(&fd, COMMENT_ATTRIBUTE) {
                Ok(()) | Err(rustix::io::Errno::NODATA) => Ok(()),
                Err(error) => Err(error.into()),
            },
            Some(text) => rustix::fs::fsetxattr(
                &fd,
                COMMENT_ATTRIBUTE,
                text.as_bytes(),
                rustix::fs::XattrFlags::empty(),
            )
            .map_err(Into::into),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "File comments require Linux",
        ))
    }
}

/// Read a file's FreeDesktop download origin URL (`user.xdg.origin.url`), when present.
pub fn read_origin_url(path: &Path) -> Option<String> {
    let raw = read_named_raw(path, ORIGIN_URL_ATTRIBUTE)?;
    let text = std::str::from_utf8(&raw).ok()?.trim();
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    Some(text.to_owned())
}

/// Read an explicit FreeDesktop MIME override (`user.mime_type`), when present and syntactically
/// valid (`type/subtype`).
pub fn read_mime_type(path: &Path) -> Option<String> {
    let raw = read_named_raw(path, MIME_TYPE_ATTRIBUTE)?;
    let text = std::str::from_utf8(&raw).ok()?.trim();
    let (top, sub) = text.split_once('/')?;
    if top.is_empty() || sub.is_empty() || sub.contains('/') {
        return None;
    }
    let valid_part = |part: &str| {
        part.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'+' | b'_'))
    };
    if !valid_part(top) || !valid_part(sub) {
        return None;
    }
    Some(text.to_ascii_lowercase())
}

#[cfg(target_os = "linux")]
fn read_named_raw(path: &Path, name: &str) -> Option<Vec<u8>> {
    let mut bytes = vec![0; 4096];
    match rustix::fs::getxattr(path, name, bytes.as_mut_slice()) {
        Ok(count) => {
            bytes.truncate(count);
            Some(bytes)
        }
        Err(_) => None,
    }
}

#[cfg(not(target_os = "linux"))]
fn read_named_raw(_: &Path, _: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(target_os = "linux")]
fn list_user_xattrs_fd(source: &std::fs::File) -> io::Result<Vec<std::ffi::CString>> {
    let mut size = INITIAL_XATTR_LIST_SIZE;
    loop {
        let mut buf = vec![0u8; size];
        match rustix::fs::flistxattr(source, buf.as_mut_slice()) {
            Ok(count) => {
                buf.truncate(count);
                return Ok(parse_user_xattr_names(&buf));
            }
            Err(rustix::io::Errno::RANGE) if size < MAX_XATTR_LIST_SIZE => {
                size = (size * 2).min(MAX_XATTR_LIST_SIZE);
            }
            Err(rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::NOSYS) => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(target_os = "linux")]
fn list_user_xattrs_path(source: &Path) -> io::Result<Vec<std::ffi::CString>> {
    let mut size = INITIAL_XATTR_LIST_SIZE;
    loop {
        let mut buf = vec![0u8; size];
        match rustix::fs::listxattr(source, buf.as_mut_slice()) {
            Ok(count) => {
                buf.truncate(count);
                return Ok(parse_user_xattr_names(&buf));
            }
            Err(rustix::io::Errno::RANGE) if size < MAX_XATTR_LIST_SIZE => {
                size = (size * 2).min(MAX_XATTR_LIST_SIZE);
            }
            Err(rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::NOSYS) => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(target_os = "linux")]
fn parse_user_xattr_names(raw: &[u8]) -> Vec<std::ffi::CString> {
    raw.split(|&b| b == 0)
        .filter(|name| name.starts_with(b"user.") && name.len() > b"user.".len())
        .filter_map(|name| std::ffi::CString::new(name).ok())
        .collect()
}

/// Copy all portable `user.*` extended attributes from the validated open file descriptor
/// (including `user.xdg.tags`, `user.xdg.origin.url`, `user.xdg.comment`, and `user.mime_type`).
///
/// # Errors
/// Returns attribute I/O errors other than absent or unsupported filesystem attributes.
#[cfg(target_os = "linux")]
pub fn copy_from(source: &std::fs::File, destination: &Path) -> io::Result<()> {
    let names = list_user_xattrs_fd(source)?;
    if names.is_empty() {
        return Ok(());
    }
    let dest_fd = open_target(destination)?;
    let mut value = vec![0u8; MAX_ATTRIBUTE_SIZE];
    for name in &names {
        let count = match rustix::fs::fgetxattr(source, name.as_c_str(), value.as_mut_slice()) {
            Ok(count) => count,
            Err(rustix::io::Errno::NODATA | rustix::io::Errno::OPNOTSUPP) => continue,
            Err(error) => return Err(error.into()),
        };
        match rustix::fs::fsetxattr(
            &dest_fd,
            name.as_c_str(),
            &value[..count],
            rustix::fs::XattrFlags::empty(),
        ) {
            Ok(()) => {}
            Err(rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::NOSYS) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Copy all portable `user.*` extended attributes from the validated open file descriptor.
///
/// # Errors
/// Returns attribute I/O errors other than absent or unsupported filesystem attributes.
#[cfg(not(target_os = "linux"))]
pub fn copy_from(_source: &std::fs::File, _destination: &Path) -> io::Result<()> {
    Ok(())
}

/// Preserve all portable `user.*` extended attributes during a path-based copy; filesystems
/// without xattr support are skipped cleanly.
///
/// # Errors
/// Returns errors reading or writing attributes, except unsupported filesystem attributes.
#[cfg(target_os = "linux")]
pub fn copy(source: &Path, destination: &Path) -> io::Result<()> {
    let names = list_user_xattrs_path(source)?;
    if names.is_empty() {
        return Ok(());
    }
    let mut value = vec![0u8; MAX_ATTRIBUTE_SIZE];
    for name in &names {
        let count = match rustix::fs::getxattr(source, name.as_c_str(), value.as_mut_slice()) {
            Ok(count) => count,
            Err(rustix::io::Errno::NODATA | rustix::io::Errno::OPNOTSUPP) => continue,
            Err(error) => return Err(error.into()),
        };
        match rustix::fs::setxattr(
            destination,
            name.as_c_str(),
            &value[..count],
            rustix::fs::XattrFlags::empty(),
        ) {
            Ok(()) => {}
            Err(rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::NOSYS) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Preserve all portable `user.*` extended attributes during a path-based copy.
///
/// # Errors
/// Returns errors reading or writing attributes, except unsupported filesystem attributes.
#[cfg(not(target_os = "linux"))]
pub fn copy(_source: &Path, _destination: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
