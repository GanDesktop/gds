//! Active freedesktop trash `info/` directory discovery and live watching.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::Sender;

use super::directory_sizes;

/// Mount filesystem types skipped when probing top-level trash directories so
/// discovery never stalls on remote/automount filesystems or wastes syscalls
/// on kernel pseudo-filesystems.
#[cfg(any(target_os = "linux", test))]
const SKIPPED_MOUNT_FS_TYPES: &[&[u8]] = &[
    b"9p",
    b"afs",
    b"autofs",
    b"binfmt_misc",
    b"bpf",
    b"ceph",
    b"cgroup",
    b"cgroup2",
    b"cifs",
    b"configfs",
    b"debugfs",
    b"devpts",
    b"devtmpfs",
    b"efivarfs",
    b"fuse.gvfsd-fuse",
    b"fuse.rclone",
    b"fuse.sshfs",
    b"fusectl",
    b"glusterfs",
    b"hugetlbfs",
    b"lustre",
    b"mqueue",
    b"nfs",
    b"nfs4",
    b"nsfs",
    b"pipefs",
    b"proc",
    b"pstore",
    b"rpc_pipefs",
    b"securityfs",
    b"selinuxfs",
    b"smb3",
    b"sockfs",
    b"sysfs",
    b"tracefs",
];

#[cfg(any(target_os = "linux", test))]
const MOUNTINFO_ESCAPES: &[(&[u8], u8)] = &[
    (br"\040", b' '),
    (br"\011", b'\t'),
    (br"\012", b'\n'),
    (br"\134", b'\\'),
];

/// Return active freedesktop trash `info/` directories to watch for live changes.
///
/// Always includes the user's home trash `info/` directory
/// (`$XDG_DATA_HOME/Trash/info`, ignoring relative `XDG_DATA_HOME` per the XDG
/// Base Directory Specification and falling back to `~/.local/share/Trash/info`),
/// even before it exists on disk, plus any existing local top-level mount trash
/// `info/` directories (`$topdir/.Trash/$uid/info` and `$topdir/.Trash-$uid/info`).
pub fn watch_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = BTreeSet::new();

    if let Some(home_info) = home_trash_info_dir()
        && seen.insert(home_info.clone())
    {
        dirs.push(home_info);
    }

    for mount_info in discover_mount_trash_info_dirs() {
        if seen.insert(mount_info.clone()) {
            dirs.push(mount_info);
        }
    }

    dirs
}

/// Resolve `$XDG_DATA_HOME/Trash/info`, ignoring relative `XDG_DATA_HOME`
/// values per the XDG Base Directory Specification and falling back to
/// `$HOME/.local/share/Trash/info`.
pub(super) fn resolve_home_trash_info(
    xdg_data_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(raw) = xdg_data_home {
        let dir = Path::new(raw);
        if dir.is_absolute() {
            return Some(dir.join("Trash").join("info"));
        }
    }
    if let Some(raw) = home {
        let dir = Path::new(raw);
        if dir.is_absolute() {
            return Some(dir.join(".local").join("share").join("Trash").join("info"));
        }
    }
    None
}

fn home_trash_info_dir() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_DATA_HOME");
    let home = std::env::var_os("HOME");
    resolve_home_trash_info(xdg.as_deref(), home.as_deref()).or_else(|| {
        directories::BaseDirs::new().map(|dirs| dirs.data_local_dir().join("Trash").join("info"))
    })
}

fn discover_mount_trash_info_dirs() -> Vec<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let Ok(mountinfo) = fs::read("/proc/self/mountinfo") else {
            return Vec::new();
        };
        let uid = nix::unistd::Uid::current().as_raw();
        mount_trash_info_dirs_from_mountinfo(&mountinfo, uid)
    }
    #[cfg(target_os = "freebsd")]
    {
        let Ok(folders) = trash::os_limited::trash_folders() else {
            return Vec::new();
        };
        let mut dirs: Vec<PathBuf> = folders
            .into_iter()
            .filter(|folder| folder.is_absolute())
            .map(|folder| folder.join("info"))
            .filter(|info| info.is_dir())
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        Vec::new()
    }
}

/// Extract existing valid per-mount trash `info/` directories from Linux
/// `/proc/self/mountinfo` bytes for `uid`.
#[cfg(any(target_os = "linux", test))]
pub(super) fn mount_trash_info_dirs_from_mountinfo(mountinfo: &[u8], uid: u32) -> Vec<PathBuf> {
    let mut topdirs = BTreeSet::new();
    for line in mountinfo.split(|&byte| byte == b'\n') {
        let fields: Vec<_> = line.split(|&byte| byte == b' ').collect();
        let Some(separator) = fields.iter().position(|field| *field == b"-") else {
            continue;
        };
        let Some(&fstype) = fields.get(separator + 1) else {
            continue;
        };
        if SKIPPED_MOUNT_FS_TYPES.contains(&fstype) {
            continue;
        }
        let Some(&raw_mount) = fields.get(4) else {
            continue;
        };
        let mount = decode_mount_path(raw_mount);
        if mount.is_absolute() {
            topdirs.insert(mount);
        }
    }

    let mut result = Vec::new();
    for topdir in topdirs {
        collect_topdir_trash_info(&topdir, uid, &mut result);
    }
    result
}

#[cfg(any(target_os = "linux", test))]
fn collect_topdir_trash_info(topdir: &Path, uid: u32, out: &mut Vec<PathBuf>) {
    use std::os::unix::fs::PermissionsExt as _;

    // 1. Administrator-created shared `$topdir/.Trash/$uid` (must be a sticky non-symlink dir).
    const S_ISVTX: u32 = 0o1000;
    let shared_trash = topdir.join(".Trash");
    if let Ok(meta) = fs::symlink_metadata(&shared_trash)
        && !meta.file_type().is_symlink()
        && meta.is_dir()
        && (meta.permissions().mode() & S_ISVTX) != 0
    {
        let info = shared_trash.join(uid.to_string()).join("info");
        if info.is_dir() {
            out.push(info);
        }
    }

    // 2. Per-user `$topdir/.Trash-$uid` (must not be a symlink).
    let user_trash = topdir.join(format!(".Trash-{uid}"));
    if let Ok(meta) = fs::symlink_metadata(&user_trash)
        && !meta.file_type().is_symlink()
        && meta.is_dir()
    {
        let info = user_trash.join("info");
        if info.is_dir() {
            out.push(info);
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn decode_mount_path(raw: &[u8]) -> PathBuf {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt as _};
    PathBuf::from(OsString::from_vec(unescape_mountinfo(raw)))
}

#[cfg(any(target_os = "linux", test))]
fn unescape_mountinfo(mut bytes: &[u8]) -> Vec<u8> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

impl DirectoryIdentity {
    fn of(path: &Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt as _;

        let metadata = fs::metadata(path).ok()?;
        if !metadata.is_dir() {
            return None;
        }
        Some(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

/// Resolve the existing canonical prefix even when the watched trash directory
/// has not been created yet, so symlink retargeting along the path is followed.
fn resolved_target(path: &Path) -> Option<PathBuf> {
    path.ancestors().find_map(|ancestor| {
        let resolved = fs::canonicalize(ancestor).ok()?;
        Some(resolved.join(path.strip_prefix(ancestor).ok()?))
    })
}

/// Live filesystem watcher for freedesktop trash `info/` directories and their
/// sibling `directorysizes` caches.
///
/// Unlike single-file watches (`gds::service::desktop::watch::Sources`), this
/// watches each existing `info/` directory itself for child `.trashinfo`
/// creations, modifications, renames, and deletions while also watching
/// existing ancestors so missing or replaced `$trash/info` directories are
/// picked up automatically on [`Self::refresh`].
pub struct TrashWatch {
    requested: Vec<PathBuf>,
    info_targets: Vec<PathBuf>,
    directories: BTreeMap<PathBuf, DirectoryIdentity>,
    sender: Sender<()>,
    watcher: Option<RecommendedWatcher>,
}

impl TrashWatch {
    pub fn new(sender: Sender<()>) -> Self {
        Self {
            requested: Vec::new(),
            info_targets: Vec::new(),
            directories: BTreeMap::new(),
            sender,
            watcher: None,
        }
    }

    /// Replace the watched trash `info/` directories and install platform watches.
    ///
    /// # Errors
    /// Reports an error only if creating the platform watcher fails or no
    /// requested directory (nor any of its ancestors) could be watched.
    pub fn set_directories(&mut self, directories: Vec<PathBuf>) -> notify::Result<()> {
        self.requested = directories;
        self.refresh()
    }

    /// Reconcile watches after directories or symlinks along any trash `info/`
    /// path are created, removed, or replaced.
    ///
    /// # Errors
    /// Reports an error only if creating the platform watcher fails or no
    /// requested directory (nor any of its ancestors) could be watched; the
    /// previous valid watcher is retained on failure.
    pub fn refresh(&mut self) -> notify::Result<()> {
        let info_targets: BTreeSet<PathBuf> = self
            .requested
            .iter()
            .flat_map(|path| std::iter::once(path.clone()).chain(resolved_target(path)))
            .collect();
        let size_cache_targets: Vec<PathBuf> = info_targets
            .iter()
            .filter_map(|info_dir| {
                info_dir
                    .parent()
                    .map(|root| root.join(directory_sizes::FILE_NAME))
            })
            .collect();

        let mut directories = BTreeMap::new();
        for target in &info_targets {
            // Walk from `target` itself (not `skip(1)`) so existing `info/`
            // directories are watched directly for `.trashinfo` child events,
            // while ancestors catch creation/replacement of missing `Trash/info`.
            for ancestor in target.ancestors() {
                let Some(identity) = DirectoryIdentity::of(ancestor) else {
                    continue;
                };
                directories.insert(ancestor.to_path_buf(), identity);
            }
        }

        let info_targets_vec: Vec<PathBuf> = info_targets.into_iter().collect();
        if info_targets_vec == self.info_targets && directories == self.directories {
            return Ok(());
        }

        let filter_infos = info_targets_vec.clone();
        let filter_caches = size_cache_targets;
        let sender = self.sender.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event) if matches_event(&event, &filter_infos, &filter_caches) => {
                    let _ = sender.try_send(());
                }
                Err(error) => tracing::warn!(%error, "trash watcher error"),
                _ => {}
            })?;

        let mut active_directories = BTreeMap::new();
        let mut last_error = None;
        for (directory, identity) in &directories {
            match watcher.watch(directory, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    active_directories.insert(directory.clone(), *identity);
                }
                Err(error) => {
                    if matches!(error.kind, notify::ErrorKind::PathNotFound) {
                        tracing::debug!(
                            ?directory,
                            %error,
                            "skipping vanished trash watch directory"
                        );
                    } else {
                        tracing::debug!(
                            ?directory,
                            %error,
                            "skipping inaccessible trash watch directory"
                        );
                        last_error = Some(error);
                    }
                }
            }
        }

        if active_directories.is_empty()
            && let Some(error) = last_error
        {
            return Err(error);
        }

        self.info_targets = info_targets_vec;
        self.directories = active_directories;
        self.watcher = Some(watcher);
        Ok(())
    }
}

fn matches_event(
    event: &notify::Event,
    info_targets: &[PathBuf],
    size_cache_targets: &[PathBuf],
) -> bool {
    if event.need_rescan() {
        return true;
    }
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return false;
    }
    event.paths.iter().any(|path| {
        info_targets
            .iter()
            .any(|info_dir| path.starts_with(info_dir) || info_dir.starts_with(path))
            || size_cache_targets.iter().any(|cache| path == cache)
    })
}
