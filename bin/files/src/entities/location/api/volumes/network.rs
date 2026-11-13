//! Mounted network folder discovery from kernel mount tables and GVfs runtime directories.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

const NETWORK_FILESYSTEM_KINDS: &[&[u8]] = &[
    b"nfs",
    b"nfs4",
    b"cifs",
    b"smb3",
    b"fuse.sshfs",
    b"fuse.rclone",
    b"fuse.gvfsd-fuse",
];

const GVFS_NETWORK_PREFIXES: &[&[u8]] = &[
    b"smb-share:",
    b"sftp:",
    b"ftp:",
    b"dav:",
    b"davs:",
    b"afp-volume:",
    b"nfs:",
];

const MOUNTINFO_ESCAPES: &[(&[u8], u8)] = &[
    (br"\040", b' '),
    (br"\011", b'\t'),
    (br"\012", b'\n'),
    (br"\134", b'\\'),
];

#[derive(Debug, thiserror::Error)]
pub enum NetworkMountError {
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
    #[error("cannot read network locations at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[cfg(not(target_os = "linux"))]
    #[error("network mount discovery is not supported on this platform")]
    Unsupported,
}

/// Discover mounted network folders from `/proc/self/mountinfo` and GVfs runtime directories.
///
/// # Errors
/// Reports inaccessible mount information or an unreadable GVfs mount directory.
pub fn read_network_mounts() -> Result<Vec<PathBuf>, NetworkMountError> {
    #[cfg(target_os = "linux")]
    {
        let path = PathBuf::from("/proc/self/mountinfo");
        let mounts = fs::read(&path).map_err(|source| NetworkMountError::Read { path, source })?;
        discover_network_mounts(&mounts)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(NetworkMountError::Unsupported)
    }
}

/// Parse `mountinfo` bytes and expand any `fuse.gvfsd-fuse` runtime directories into individual
/// network share paths.
///
/// # Errors
/// Reports an unreadable GVfs mount directory or directory entry.
pub(super) fn discover_network_mounts(mountinfo: &[u8]) -> Result<Vec<PathBuf>, NetworkMountError> {
    let mut paths = Vec::new();
    for (path, kind) in mounts_from(mountinfo) {
        if kind == b"fuse.gvfsd-fuse" {
            collect_gvfs_network_entries(&path, &mut paths)?;
        } else {
            paths.push(path);
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn collect_gvfs_network_entries(
    directory: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), NetworkMountError> {
    let entries = fs::read_dir(directory).map_err(|source| NetworkMountError::Read {
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| NetworkMountError::Read {
            path: directory.to_path_buf(),
            source,
        })?;
        let name = entry.file_name();
        if is_gvfs_network_entry(name.as_encoded_bytes()) {
            paths.push(entry.path());
        }
    }
    Ok(())
}

pub(super) fn is_gvfs_network_entry(name: &[u8]) -> bool {
    GVFS_NETWORK_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

pub(super) fn mounts_from(input: &[u8]) -> Vec<(PathBuf, Vec<u8>)> {
    // Field 5 is the mount path; optional fields end at "-" before the filesystem type.
    // https://man7.org/linux/man-pages/man5/proc_pid_mountinfo.5.html
    input
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            let fields: Vec<_> = line.split(|byte| *byte == b' ').collect();
            let separator = fields.iter().position(|field| *field == b"-")?;
            let kind = *fields.get(separator + 1)?;
            if !NETWORK_FILESYSTEM_KINDS.contains(&kind) {
                return None;
            }
            let path = decode_mount_path(fields.get(4)?);
            path.is_absolute().then(|| (path, kind.to_vec()))
        })
        .collect()
}

#[cfg(unix)]
fn decode_mount_path(raw: &[u8]) -> PathBuf {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    PathBuf::from(OsString::from_vec(unescape(raw)))
}

#[cfg(not(unix))]
fn decode_mount_path(raw: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&unescape(raw)).into_owned())
}

pub(super) fn unescape(mut bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());
    while let Some((&byte, rest)) = bytes.split_first() {
        let escape = MOUNTINFO_ESCAPES
            .iter()
            .find_map(|&(prefix, value)| bytes.strip_prefix(prefix).map(|tail| (tail, value)));
        if let Some((tail, value)) = escape {
            result.push(value);
            bytes = tail;
        } else {
            result.push(byte);
            bytes = rest;
        }
    }
    result
}
