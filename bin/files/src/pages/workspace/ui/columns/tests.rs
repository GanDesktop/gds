use crate::entities::location::Location;
use std::path::Path;

use crate::entities::file::columns::{Column, Layout};

use super::{Editor, Scope, sort_for};
use crate::entities::file::sorting::{Direction, Key, Sort};
use crate::entities::location::places::Places;

fn layout_of(columns: &[Column]) -> Layout {
    Layout::new(columns.iter().copied())
}

fn editor_with(columns: &[Column]) -> Editor {
    let mut editor = Editor::new();
    editor.load(Path::new("/a"), layout_of(columns), false);
    editor
}

#[test]
fn opening_always_starts_at_the_current_folder() {
    let mut editor = Editor::new();
    editor.load(
        Path::new("/a"),
        layout_of(&[Column::Name, Column::Size]),
        false,
    );
    editor.scope = Scope::Global;
    editor.load(
        Path::new("/b"),
        layout_of(&[Column::Name, Column::Owner]),
        true,
    );
    assert_eq!(editor.scope, Scope::Folder);
    assert!(editor.has_override());
    let request = editor
        .request()
        .expect("the second opening is still loaded");
    assert_eq!(request.location, Location::from(Path::new("/b")));
    assert_eq!(
        request.layout.columns(),
        [Column::Name, Column::Owner].as_slice()
    );
}

#[test]
fn hidden_columns_are_listed_after_the_visible_ones() {
    let mut editor = Editor::new();
    editor.load(
        Path::new("/a"),
        layout_of(&[Column::Name, Column::Size]),
        true,
    );
    editor.set_visible(Column::Modified, true);
    editor.set_visible(Column::Size, false);
    let rows = editor.rows();
    let visible: Vec<_> = rows
        .iter()
        .filter(|row| row.visible)
        .map(|row| row.column)
        .collect();
    assert_eq!(
        visible.as_slice(),
        [Column::Name, Column::Modified].as_slice()
    );
    let hidden: Vec<_> = rows
        .iter()
        .filter(|row| !row.visible)
        .map(|row| row.column)
        .collect();
    assert_eq!(hidden.first(), Some(&Column::Type));
    assert_eq!(hidden.len(), Column::ALL.len() - 2);
}

#[test]
fn name_cannot_be_hidden_and_moves_stay_inside_the_visible_order() {
    let mut editor = Editor::new();
    editor.load(
        Path::new("/a"),
        layout_of(&[Column::Name, Column::Type, Column::Size]),
        false,
    );
    assert!(!editor.set_visible(Column::Name, false));
    assert!(editor.draft.contains(Column::Name));

    assert!(editor.move_column(Column::Size, -1));
    assert_eq!(
        editor.draft.columns(),
        [Column::Name, Column::Size, Column::Type].as_slice()
    );
    assert!(!editor.move_column(Column::Name, -1));
    assert!(editor.move_column(Column::Name, 9));
    assert_eq!(
        editor.draft.columns(),
        [Column::Size, Column::Type, Column::Name].as_slice()
    );
    // Hidden columns have no visible position to move from.
    editor.set_visible(Column::Modified, false);
    assert!(!editor.move_column(Column::Modified, 1));
}

#[test]
fn confirming_a_draft_yields_a_normalized_layout() {
    let mut editor = Editor::new();
    editor.load(
        Path::new("/a"),
        layout_of(&[Column::Name, Column::Type]),
        false,
    );
    editor.set_visible(Column::Owner, true);
    editor.set_visible(Column::Owner, true);
    assert!(editor.move_column(Column::Owner, -2));
    let request = editor.request().expect("the editor is loaded");
    assert_eq!(request.scope, Scope::Folder);
    assert_eq!(
        request.layout.columns(),
        [Column::Owner, Column::Name, Column::Type].as_slice()
    );
    assert!(request.layout.contains(Column::Name));
}

#[test]
fn folder_override_decides_the_sort_fallback_per_exact_path() {
    let mut places = Places::default();
    let global = layout_of(&[Column::Name, Column::Size]);
    places.set_columns(Path::new("/a"), layout_of(&[Column::Name]));
    let by_size = Sort {
        key: Key::Size,
        direction: Direction::Descending,
    };
    places.set_default_columns(global);
    places.set_sort(Location::from(Path::new("/a")), by_size);
    places.set_sort(Location::from(Path::new("/b")), by_size);
    places.set_sort(Location::from(Path::new("/a/child")), by_size);
    assert_eq!(
        sort_for(&Location::from(Path::new("/a")), &places),
        Sort::default()
    );
    assert_eq!(sort_for(&Location::from(Path::new("/b")), &places), by_size);
    // The override is exact: a child folder keeps using the global layout.
    assert_eq!(
        sort_for(&Location::from(Path::new("/a/child")), &places),
        by_size
    );
}

#[test]
fn dragging_a_column_above_a_row_inserts_it_before_that_row() {
    let mut editor = editor_with(&[Column::Name, Column::Type, Column::Size]);
    let size = editor.drag_payload(Column::Size);
    assert!(editor.drop_before(&size, Column::Name));
    assert_eq!(
        editor.draft.columns(),
        [Column::Size, Column::Name, Column::Type].as_slice()
    );
}

#[test]
fn dragging_down_inserts_before_the_target_and_never_scrambles_other_columns() {
    let mut editor = editor_with(&[Column::Name, Column::Type, Column::Size, Column::Modified]);
    let name = editor.drag_payload(Column::Name);
    assert!(editor.drop_before(&name, Column::Size));
    assert_eq!(
        editor.draft.columns(),
        [Column::Type, Column::Name, Column::Size, Column::Modified].as_slice()
    );
}

#[test]
fn dropping_a_column_just_above_its_lower_neighbour_is_a_no_op() {
    let mut editor = editor_with(&[Column::Name, Column::Type, Column::Size]);
    let name = editor.drag_payload(Column::Name);
    assert!(!editor.drop_before(&name, Column::Type));
    assert_eq!(
        editor.draft.columns(),
        [Column::Name, Column::Type, Column::Size].as_slice()
    );
}

#[test]
fn the_end_lane_appends_and_name_can_be_reordered_there() {
    let mut editor = editor_with(&[Column::Name, Column::Type, Column::Size]);
    let name = editor.drag_payload(Column::Name);
    assert!(editor.drop_at_end(&name));
    assert_eq!(
        editor.draft.columns(),
        [Column::Type, Column::Size, Column::Name].as_slice()
    );
    assert!(editor.draft.contains(Column::Name));
}

#[test]
fn dropping_a_column_on_itself_keeps_the_order() {
    let mut editor = editor_with(&[Column::Name, Column::Type, Column::Size]);
    let column_type = editor.drag_payload(Column::Type);
    assert!(!editor.drop_before(&column_type, Column::Type));
    assert_eq!(
        editor.draft.columns(),
        [Column::Name, Column::Type, Column::Size].as_slice()
    );
}

#[test]
fn hidden_columns_cannot_be_dragged_into_the_visible_list() {
    let mut editor = editor_with(&[Column::Name, Column::Size]);
    let modified = editor.drag_payload(Column::Modified);
    assert!(!editor.drop_before(&modified, Column::Name));
    assert!(!editor.drop_at_end(&modified));
    assert_eq!(
        editor.draft.columns(),
        [Column::Name, Column::Size].as_slice()
    );
}

#[test]
fn a_drag_from_another_editor_is_rejected() {
    let mut target = editor_with(&[Column::Name, Column::Type]);
    let source = editor_with(&[Column::Name, Column::Size]);
    let foreign = source.drag_payload(Column::Size);
    assert!(!target.drop_before(&foreign, Column::Name));
    assert!(!target.drop_at_end(&foreign));
    assert_eq!(
        target.draft.columns(),
        [Column::Name, Column::Type].as_slice()
    );
}

#[test]
fn showing_a_hidden_column_appends_it_after_the_visible_order() {
    let mut editor = editor_with(&[Column::Name, Column::Size]);
    assert!(editor.set_visible(Column::Type, true));
    assert_eq!(
        editor.draft.columns(),
        [Column::Name, Column::Size, Column::Type].as_slice()
    );
}

#[test]
fn cancelling_throws_the_draft_away_without_touching_the_applied_layout() {
    let mut editor = editor_with(&[Column::Name, Column::Type]);
    let column_type = editor.drag_payload(Column::Type);
    assert!(editor.drop_before(&column_type, Column::Name));
    editor.set_visible(Column::Size, true);
    editor.scope = Scope::Global;

    editor.finish();

    assert!(!editor.is_open());
    assert!(editor.request().is_none());
    editor.load(
        Path::new("/a"),
        layout_of(&[Column::Name, Column::Type]),
        true,
    );
    assert_eq!(editor.scope, Scope::Folder);
    assert_eq!(
        editor.draft.columns(),
        [Column::Name, Column::Type].as_slice()
    );
}
