use super::Target;
use std::path::PathBuf;

#[test]
fn item_targets_group_by_parent_and_deduplicate_selections() {
    let targets = Target::items(
        ["/tmp/a", "/", "/tmp/folder", "/tmp/a"]
            .map(PathBuf::from)
            .to_vec(),
    );
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].directory, PathBuf::from("/tmp"));
    assert_eq!(
        targets[0].selection,
        [PathBuf::from("/tmp/a"), PathBuf::from("/tmp/folder")]
    );
    assert_eq!(targets[1].directory, PathBuf::from("/"));
    assert_eq!(targets[1].selection, [PathBuf::from("/")]);
}

#[test]
fn folder_targets_open_each_directory_once_without_selection() {
    let targets = Target::folders(
        ["/tmp/folder", "/", "/tmp/folder"]
            .map(PathBuf::from)
            .to_vec(),
    );
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].directory, PathBuf::from("/tmp/folder"));
    assert!(targets[0].selection.is_empty());
    assert_eq!(targets[1].directory, PathBuf::from("/"));
    assert!(targets[1].selection.is_empty());
}
