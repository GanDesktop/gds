use std::path::PathBuf;

use crate::entities::file::columns::{Column, Layout};

use super::*;

fn folder(path: &str) -> PathBuf {
    PathBuf::from(path)
}

#[test]
fn views_are_isolated_and_clearing_restores_the_global_layout() {
    let global = Layout::default();
    let first = Layout::new([Column::Name, Column::Size]);
    let second = Layout::new([Column::Name, Column::Owner, Column::Accessed]);
    let first_path = folder("/tmp/one");
    let second_path = folder("/tmp/two");
    let uncovered = folder("/tmp/three");
    let mut places = Places::default();

    places.set_columns(&first_path, first.clone());
    places.set_columns(&second_path, second.clone());
    assert_eq!(places.columns_for(&first_path), Some(&first));
    assert_eq!(places.effective_columns(&first_path), &first);
    assert_eq!(places.effective_columns(&second_path), &second);
    assert_eq!(places.effective_columns(&uncovered), &global);

    let replaced = Layout::new([Column::Name, Column::Permissions]);
    places.set_columns(&first_path, replaced.clone());
    assert_eq!(places.columns_for(&first_path), Some(&replaced));
    assert_eq!(places.columns_for(&second_path), Some(&second));
    assert_eq!(places.views.len(), 2);

    assert!(places.clear_columns(&first_path));
    assert_eq!(places.effective_columns(&first_path), &global);
    assert_eq!(places.effective_columns(&second_path), &second);
    assert!(!places.clear_columns(&first_path));
}

#[test]
fn overrides_apply_to_exact_paths_without_parent_or_child_inheritance() {
    let global = Layout::default();
    let child_layout = Layout::new([Column::Name, Column::Group]);
    let parent = folder("/tmp/project");
    let child = folder("/tmp/project/src");
    let mut places = Places::default();

    places.set_columns(&child, child_layout.clone());
    assert_eq!(places.effective_columns(&parent), &global);
    assert_eq!(places.effective_columns(&child), &child_layout);
    assert_eq!(
        places.effective_columns(&folder("/tmp/project/src2")),
        &global
    );
}
