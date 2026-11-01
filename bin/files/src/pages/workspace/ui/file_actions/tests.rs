use super::Selection;
use crate::entities::file::directory::{self, ReadOptions};
use std::path::PathBuf;

#[cfg(unix)]
#[test]
fn directory_symlinks_open_terminals_in_the_link_not_its_parent() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().expect("terminal target fixture");
    let folder = root.path().join("folder");
    std::fs::create_dir(&folder).expect("create target folder");
    std::fs::write(root.path().join("file"), "data").expect("create target file");
    symlink(&folder, root.path().join("folder-link")).expect("directory link");
    symlink(root.path().join("file"), root.path().join("file-link")).expect("file link");
    symlink(root.path().join("missing"), root.path().join("broken-link")).expect("broken link");
    let snapshot =
        directory::read_directory(root.path(), ReadOptions::default()).expect("read fixture");
    for (name, directory) in [
        ("folder-link", true),
        ("file-link", false),
        ("broken-link", false),
    ] {
        let entry = snapshot
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .expect("entry");
        assert_eq!(Selection::directory_entry(entry), directory);
        let selection = Selection {
            tab_id: 1,
            directory: root.path().into(),
            paths: vec![entry.path.clone()],
            directories_only: Selection::directory_entry(entry),
        };
        assert_eq!(
            selection.terminal_target(),
            Some(if directory {
                entry.path.clone()
            } else {
                root.path().into()
            })
        );
    }
}

#[test]
fn ambiguous_terminal_targets_do_not_choose_the_first_selected_directory() {
    let selection = Selection {
        tab_id: 1,
        directory: PathBuf::from("/work"),
        paths: vec![PathBuf::from("/work/a"), PathBuf::from("/work/b")],
        directories_only: true,
    };
    assert_eq!(selection.terminal_target(), None);
}
