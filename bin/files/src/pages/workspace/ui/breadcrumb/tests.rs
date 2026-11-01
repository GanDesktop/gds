use std::path::{Path, PathBuf};

use super::segments;

#[test]
fn absolute_path_uses_one_block_per_parent() {
    let segments = segments(Path::new("/mnt/st2000/assets"));
    let labels: Vec<_> = segments
        .iter()
        .map(|segment| segment.label.as_str())
        .collect();
    assert_eq!(labels, ["/", "mnt/", "st2000/", "assets"]);
    let paths: Vec<_> = segments
        .iter()
        .map(|segment| segment.path.clone())
        .collect();
    assert_eq!(
        paths,
        [
            PathBuf::from("/"),
            PathBuf::from("/mnt"),
            PathBuf::from("/mnt/st2000"),
            PathBuf::from("/mnt/st2000/assets"),
        ]
    );
    assert!(segments.last().is_some_and(|segment| segment.current));
    assert!(segments[..3].iter().all(|segment| !segment.current));
}

#[test]
fn root_is_a_single_current_block() {
    let segments = segments(Path::new("/"));
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].label, "/");
    assert_eq!(segments[0].path, Path::new("/"));
    assert!(segments[0].current);
}

#[test]
fn trailing_separator_does_not_add_an_empty_block() {
    let labels: Vec<_> = segments(Path::new("/mnt/assets/"))
        .into_iter()
        .map(|segment| segment.label)
        .collect();
    assert_eq!(labels, ["/", "mnt/", "assets"]);
}
