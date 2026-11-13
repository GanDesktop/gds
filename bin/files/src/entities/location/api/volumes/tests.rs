use std::{fs, path::PathBuf};

use super::network::{discover_network_mounts, is_gvfs_network_entry, mounts_from, unescape};

#[test]
fn unescape_decodes_mountinfo_octal_sequences() {
    let raw = br"/mnt/Share\040With\011Tab\012Line\134Slash";
    assert_eq!(
        unescape(raw),
        b"/mnt/Share With\tTab\nLine\\Slash".as_slice()
    );
}

#[test]
fn mounts_from_filters_network_kinds_and_unescapes_paths() {
    let sample = br#"29 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw
44 29 0:39 / /mnt/nas\040media rw,relatime - nfs4 10.0.0.2:/media rw
45 29 0:40 / /mnt/windows rw,relatime - cifs //server/share rw
46 29 0:41 / relative/path rw,relatime - smb3 //server/rel rw
47 29 0:42 / /run/user/1000/gvfs rw,nosuid,nodev - fuse.gvfsd-fuse gvfsd-fuse rw
"#;
    let parsed = mounts_from(sample);
    assert_eq!(
        parsed,
        vec![
            (PathBuf::from("/mnt/nas media"), b"nfs4".to_vec()),
            (PathBuf::from("/mnt/windows"), b"cifs".to_vec()),
            (
                PathBuf::from("/run/user/1000/gvfs"),
                b"fuse.gvfsd-fuse".to_vec()
            ),
        ]
    );
}

#[test]
fn gvfs_entry_filter_recognizes_supported_network_schemes() {
    assert!(is_gvfs_network_entry(
        b"smb-share:server=nas.local,share=docs"
    ));
    assert!(is_gvfs_network_entry(b"sftp:host=example.com,user=alice"));
    assert!(!is_gvfs_network_entry(b"mtp:host=phone"));
    assert!(!is_gvfs_network_entry(b"gphoto2:host=camera"));
}

#[test]
fn discover_network_mounts_expands_gvfs_and_deduplicates() {
    let temp = tempfile::tempdir().expect("create temporary gvfs directory");
    let sftp_dir = temp.path().join("sftp:host=box.local");
    let smb_dir = temp.path().join("smb-share:server=box.local,share=pub");
    let ignored_dir = temp.path().join("mtp:host=android");
    fs::create_dir(&sftp_dir).expect("create sftp mount entry");
    fs::create_dir(&smb_dir).expect("create smb mount entry");
    fs::create_dir(&ignored_dir).expect("create non-network gvfs entry");

    let gvfs_path = temp.path().to_string_lossy().replace(' ', r"\040");
    let mountinfo = format!(
        "51 29 0:44 / /mnt/nfs rw - nfs4 10.0.0.1:/export rw\n\
         52 29 0:44 / /mnt/nfs rw - nfs4 10.0.0.1:/export rw\n\
         53 29 0:45 / {gvfs_path} rw - fuse.gvfsd-fuse gvfsd-fuse rw\n"
    );

    let discovered =
        discover_network_mounts(mountinfo.as_bytes()).expect("discover network mounts");
    let mut expected = vec![PathBuf::from("/mnt/nfs"), sftp_dir, smb_dir];
    expected.sort();
    assert_eq!(discovered, expected);
}
