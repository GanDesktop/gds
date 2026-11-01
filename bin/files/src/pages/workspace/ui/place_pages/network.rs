//! Mounted network folders, read off the UI thread without scanning their contents.

use std::{fs, io, path::PathBuf};

#[derive(Default)]
pub(in super::super) struct State {
    pub paths: Vec<PathBuf>,
    pub loaded: bool,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub(in super::super) enum Error {
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

/// # Errors
/// Reports inaccessible mount information or an unreadable GVfs mount directory.
pub(super) fn read() -> Result<Vec<PathBuf>, Error> {
    #[cfg(target_os = "linux")]
    {
        let path = PathBuf::from("/proc/self/mountinfo");
        let mounts = fs::read(&path).map_err(|source| Error::Read { path, source })?;
        let mut paths = Vec::new();
        for (path, kind) in mounts_from(&mounts) {
            if kind == b"fuse.gvfsd-fuse" {
                let entries = fs::read_dir(&path).map_err(|source| Error::Read {
                    path: path.clone(),
                    source,
                })?;
                for entry in entries {
                    let entry = entry.map_err(|source| Error::Read {
                        path: path.clone(),
                        source,
                    })?;
                    let name = entry.file_name();
                    let name = name.as_encoded_bytes();
                    if [
                        b"smb-share:".as_slice(),
                        b"sftp:",
                        b"ftp:",
                        b"dav:",
                        b"davs:",
                        b"afp-volume:",
                        b"nfs:",
                    ]
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
                    {
                        paths.push(entry.path());
                    }
                }
            } else {
                paths.push(path);
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::Unsupported)
    }
}

#[cfg(target_os = "linux")]
fn mounts_from(input: &[u8]) -> Vec<(PathBuf, Vec<u8>)> {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    // Field 5 is the mount path; optional fields end at "-" before the type.
    // https://man7.org/linux/man-pages/man5/proc_pid_mountinfo.5.html
    input
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            let fields: Vec<_> = line.split(|byte| *byte == b' ').collect();
            let separator = fields.iter().position(|field| *field == b"-")?;
            let kind = *fields.get(separator + 1)?;
            if !matches!(
                kind,
                b"nfs"
                    | b"nfs4"
                    | b"cifs"
                    | b"smb3"
                    | b"fuse.sshfs"
                    | b"fuse.rclone"
                    | b"fuse.gvfsd-fuse"
            ) {
                return None;
            }
            let path = PathBuf::from(OsString::from_vec(unescape(fields.get(4)?)));
            path.is_absolute().then(|| (path, kind.to_vec()))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn unescape(mut bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());
    while let Some((&byte, rest)) = bytes.split_first() {
        let escape = [
            (br"\040".as_slice(), b' '),
            (br"\011", b'\t'),
            (br"\012", b'\n'),
            (br"\134", b'\\'),
        ]
        .into_iter()
        .find_map(|(prefix, value)| bytes.strip_prefix(prefix).map(|tail| (tail, value)));
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

#[cfg(test)]
mod tests;
