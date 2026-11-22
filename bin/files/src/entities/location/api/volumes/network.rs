//! Mounted network folder discovery from kernel mount tables and GVfs runtime directories.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[cfg(any(target_os = "linux", test))]
const NETWORK_FILESYSTEM_KINDS: &[&[u8]] = &[
    b"nfs",
    b"nfs4",
    b"cifs",
    b"smb3",
    b"fuse.sshfs",
    b"fuse.rclone",
    b"fuse.gvfsd-fuse",
];

#[cfg(any(target_os = "freebsd", test))]
const FREEBSD_NETWORK_FILESYSTEM_KINDS: &[&[u8]] =
    &[b"nfs", b"smbfs", b"fusefs.sshfs", b"fusefs.rclone"];

#[cfg(any(target_os = "linux", target_os = "freebsd", test))]
const GVFS_NETWORK_PREFIXES: &[&[u8]] = &[
    b"smb-share:",
    b"sftp:",
    b"ftp:",
    b"dav:",
    b"davs:",
    b"afp-volume:",
    b"nfs:",
];

#[cfg(any(target_os = "linux", test))]
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
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    #[error("network mount discovery is not supported on this platform")]
    Unsupported,
}

/// Discover mounted network folders from kernel mount tables and GVfs runtime directories.
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
    #[cfg(target_os = "freebsd")]
    {
        read_freebsd_network_mounts()
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        Err(NetworkMountError::Unsupported)
    }
}

#[cfg(target_os = "freebsd")]
fn read_freebsd_network_mounts() -> Result<Vec<PathBuf>, NetworkMountError> {
    use std::{ffi::OsStr, mem::MaybeUninit, os::unix::ffi::OsStrExt};

    // Use thread-safe `getfsstat` with a caller-owned buffer instead of `getmntinfo`,
    // whose process-global static buffer races with `trash` on FreeBSD.
    let count = unsafe { nix::libc::getfsstat(std::ptr::null_mut(), 0, nix::libc::MNT_NOWAIT) };
    if count < 0 {
        return Err(NetworkMountError::Read {
            path: PathBuf::from("getfsstat"),
            source: io::Error::last_os_error(),
        });
    }
    if count == 0 {
        return Ok(Vec::new());
    }

    let capacity = (count as usize).saturating_add(8);
    let mut mounts: Vec<MaybeUninit<nix::libc::statfs>> = Vec::with_capacity(capacity);
    let buf_bytes = capacity
        .checked_mul(std::mem::size_of::<nix::libc::statfs>())
        .and_then(|bytes| nix::libc::c_long::try_from(bytes).ok())
        .unwrap_or(nix::libc::c_long::MAX);
    let filled = unsafe {
        nix::libc::getfsstat(
            mounts.as_mut_ptr().cast::<nix::libc::statfs>(),
            buf_bytes,
            nix::libc::MNT_NOWAIT,
        )
    };
    if filled < 0 {
        return Err(NetworkMountError::Read {
            path: PathBuf::from("getfsstat"),
            source: io::Error::last_os_error(),
        });
    }
    let len = (filled as usize).min(capacity);
    unsafe {
        mounts.set_len(len);
    }

    let mut paths = Vec::new();
    for slot in &mounts {
        let stat = unsafe { slot.assume_init_ref() };
        let fstype = c_char_slice_bytes(&stat.f_fstypename);
        let mount_bytes = c_char_slice_bytes(&stat.f_mntonname);
        let path = PathBuf::from(OsStr::from_bytes(mount_bytes));
        match classify_freebsd_mount(fstype, &path) {
            Some(FreebsdMountKind::Direct) => paths.push(path),
            Some(FreebsdMountKind::Gvfs) => collect_gvfs_network_entries(&path, &mut paths)?,
            None => {}
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

#[cfg(target_os = "freebsd")]
fn c_char_slice_bytes(chars: &[nix::libc::c_char]) -> &[u8] {
    let bytes = unsafe { std::slice::from_raw_parts(chars.as_ptr().cast::<u8>(), chars.len()) };
    let nul = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    &bytes[..nul]
}

#[cfg(any(target_os = "freebsd", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FreebsdMountKind {
    Direct,
    Gvfs,
}

#[cfg(any(target_os = "freebsd", test))]
pub(super) fn classify_freebsd_mount(fstype: &[u8], path: &Path) -> Option<FreebsdMountKind> {
    if !path.is_absolute() {
        return None;
    }
    if FREEBSD_NETWORK_FILESYSTEM_KINDS.contains(&fstype) {
        return Some(FreebsdMountKind::Direct);
    }
    // FreeBSD truncates `f_fstypename` to `MFSNAMELEN - 1` (15 bytes), turning
    // `fusefs.gvfsd-fuse` into `fusefs.gvfsd-fu`; also recognize subtype-less
    // `fusefs` mounts at standard `gvfs` / `.gvfs` runtime directories.
    if fstype.starts_with(b"fusefs.gvfsd-")
        || (fstype == b"fusefs"
            && path
                .file_name()
                .is_some_and(|name| name == "gvfs" || name == ".gvfs"))
    {
        return Some(FreebsdMountKind::Gvfs);
    }
    None
}

/// Parse `mountinfo` bytes and expand any `fuse.gvfsd-fuse` runtime directories into individual
/// network share paths.
///
/// # Errors
/// Reports an unreadable GVfs mount directory or directory entry.
#[cfg(any(target_os = "linux", test))]
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

#[cfg(any(target_os = "linux", target_os = "freebsd", test))]
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

#[cfg(any(target_os = "linux", target_os = "freebsd", test))]
pub(super) fn is_gvfs_network_entry(name: &[u8]) -> bool {
    GVFS_NETWORK_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

#[cfg(any(target_os = "linux", test))]
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

#[cfg(all(unix, any(target_os = "linux", test)))]
fn decode_mount_path(raw: &[u8]) -> PathBuf {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    PathBuf::from(OsString::from_vec(unescape(raw)))
}

#[cfg(all(not(unix), test))]
fn decode_mount_path(raw: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&unescape(raw)).into_owned())
}

#[cfg(any(target_os = "linux", test))]
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
