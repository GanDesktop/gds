use super::{Item, Node, Prepared};
use std::{fs, path::PathBuf};
use tokio_util::sync::CancellationToken;

// These races must occur between the copy and source cleanup. A UI scenario
// cannot reliably stop an external writer at that boundary.
struct CopiedTree {
    root: tempfile::TempDir,
    source: PathBuf,
    destination: PathBuf,
    prepared: Prepared,
    copied: super::Copied,
}

impl CopiedTree {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("isolated transfer");
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir(&source).expect("source directory");
        fs::write(source.join("copied"), b"original").expect("source payload");
        let prepared = Prepared::new(&Item {
            source: source.clone(),
            destination: destination.clone(),
            destination_committed: false,
            committed_snapshot: None,
            result: Ok(()),
        })
        .expect("prepare copy");
        let copied = Node {
            cancel: CancellationToken::new(),
            source: source.clone(),
            destination: destination.clone(),
        }
        .copy_root()
        .expect("copy before concurrent write");
        Self {
            root,
            source,
            destination,
            prepared,
            copied,
        }
    }

    fn assert_preserved(self, changed: &str) {
        assert!(self.prepared.remove_source(&self.copied).is_err());
        assert_eq!(
            fs::read(self.source.join(changed)).expect("concurrent data survives"),
            b"concurrent data"
        );
        assert_eq!(
            fs::read(self.destination.join("copied")).expect("committed copy survives"),
            b"original"
        );
        assert!(!self.destination.join("new").exists());
    }
}

#[test]
fn cross_device_cleanup_preserves_concurrent_additions() {
    let copied = CopiedTree::new();
    fs::write(copied.source.join("new"), b"concurrent data").expect("concurrent addition");
    copied.assert_preserved("new");
}

#[test]
fn cross_device_cleanup_preserves_concurrent_edits() {
    let copied = CopiedTree::new();
    fs::write(copied.source.join("copied"), b"concurrent data").expect("concurrent edit");
    copied.assert_preserved("copied");
}

#[test]
fn cross_device_cleanup_preserves_replaced_inodes() {
    let copied = CopiedTree::new();
    fs::rename(
        copied.source.join("copied"),
        copied.root.path().join("old-inode"),
    )
    .expect("retain original inode");
    fs::write(copied.source.join("copied"), b"concurrent data").expect("concurrent replacement");
    copied.assert_preserved("copied");
}

#[test]
fn copies_file_and_directory_tags_before_restoring_readonly_permissions() {
    use gds::service::file_tags;
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().expect("isolated tempdir for tag transfer test");
    let source = root.path().join("source");
    let destination = root.path().join("copy");
    fs::create_dir(&source).expect("create source directory");
    fs::write(source.join("file"), "payload").expect("write source file");
    file_tags::write(&source, &["工作".into()]).expect("tag source directory");
    file_tags::write(&source.join("file"), &["Project/design".into()]).expect("tag source file");
    fs::set_permissions(source.join("file"), fs::Permissions::from_mode(0o444))
        .expect("set readonly file mode");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o555))
        .expect("set readonly directory mode");
    let result = Node {
        source: source.clone(),
        destination: destination.clone(),
        cancel: CancellationToken::new(),
    }
    .copy_root();
    // Make the fixture removable even if an assertion fails.
    fs::set_permissions(&source, fs::Permissions::from_mode(0o755))
        .expect("restore writable source directory mode for cleanup");
    let copied_mode = fs::metadata(&destination)
        .expect("inspect copied directory")
        .permissions()
        .mode()
        & 0o777;
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))
        .expect("restore writable destination directory mode for cleanup");
    result.expect("copy readonly tree with tags");
    assert_eq!(copied_mode, 0o555);
    assert_eq!(
        file_tags::read(&destination).expect("read copied directory tags"),
        ["工作"]
    );
    assert_eq!(
        file_tags::read(&destination.join("file")).expect("read copied file tags"),
        ["Project/design"]
    );
    assert_eq!(
        fs::metadata(destination.join("file"))
            .expect("inspect copied file")
            .permissions()
            .mode()
            & 0o777,
        0o444
    );
}

#[test]
fn cross_device_cleanup_preserves_tags_changed_after_copy() {
    let copied = CopiedTree::new();
    gds::service::file_tags::write(&copied.source.join("copied"), &["external".into()])
        .expect("modify source file tags after copy");
    assert!(copied.prepared.remove_source(&copied.copied).is_err());
    assert_eq!(
        gds::service::file_tags::read(&copied.source.join("copied"))
            .expect("read preserved source file tags"),
        ["external"]
    );
}

#[test]
fn preserves_modified_timestamps_on_files_links_and_directories() {
    use rustix::fs::{AtFlags, CWD, Timespec, Timestamps, UTIME_OMIT, utimensat};
    use std::os::unix::fs::{MetadataExt, symlink};

    let set_mtime = |path: &std::path::Path, secs: i64, nsecs: i64| {
        let stamps = Timestamps {
            last_access: Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_OMIT,
            },
            last_modification: Timespec {
                tv_sec: secs as _,
                tv_nsec: nsecs as _,
            },
        };
        utimensat(CWD, path, &stamps, AtFlags::SYMLINK_NOFOLLOW).expect("set fixture mtime");
    };

    let root = tempfile::tempdir().expect("isolated tempdir for mtime transfer test");
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source).expect("create source directory");
    fs::write(source.join("note.txt"), b"archived").expect("write source file");
    symlink("missing-target", source.join("dangling-link")).expect("create source symlink");

    set_mtime(&source.join("note.txt"), 946_684_800, 123_456_789);
    set_mtime(&source.join("dangling-link"), 978_307_200, 987_654_321);
    set_mtime(&source, 915_148_800, 555_000_111);

    let prepared = Prepared::new(&Item {
        source: source.clone(),
        destination: destination.clone(),
        destination_committed: false,
        committed_snapshot: None,
        result: Ok(()),
    })
    .expect("prepare cross-device move fixture");
    let copied = Node {
        cancel: CancellationToken::new(),
        source: source.clone(),
        destination: destination.clone(),
    }
    .copy_root()
    .expect("copy source tree");
    prepared
        .remove_source(&copied)
        .expect("remove copied source tree");

    assert!(!source.exists());
    let file_meta = fs::symlink_metadata(destination.join("note.txt")).expect("copied file meta");
    assert_eq!(file_meta.mtime(), 946_684_800);
    assert_eq!(file_meta.mtime_nsec(), 123_456_789);

    let link_meta =
        fs::symlink_metadata(destination.join("dangling-link")).expect("copied symlink meta");
    assert_eq!(link_meta.mtime(), 978_307_200);
    assert_eq!(link_meta.mtime_nsec(), 987_654_321);

    let dir_meta = fs::symlink_metadata(&destination).expect("copied directory meta");
    assert_eq!(dir_meta.mtime(), 915_148_800);
    assert_eq!(dir_meta.mtime_nsec(), 555_000_111);
}
