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
