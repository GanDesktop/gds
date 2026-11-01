#![cfg(target_os = "linux")]

use super::mounts_from;
use std::{os::unix::ffi::OsStrExt, path::PathBuf};

#[test]
fn network_mount_paths_preserve_escaped_and_non_utf8_names_and_ignore_local_disks() {
    let mounts = b"1 0 8:1 / / rw - ext4 /dev/sda1 rw\n\
2 1 0:3 / /mnt/team\\040files rw shared:3 - nfs4 server:/team rw\n\
3 1 0:4 / /mnt/raw\xff rw master:7 - cifs //server/share rw\n\
4 1 0:5 / /run/user/1000/gvfs rw - fuse.gvfsd-fuse gvfsd-fuse rw\n";
    let rows = mounts_from(mounts);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, PathBuf::from("/mnt/team files"));
    assert_eq!(rows[1].0.as_os_str().as_bytes(), b"/mnt/raw\xff");
    assert_eq!(rows[2].1, b"fuse.gvfsd-fuse");
}
