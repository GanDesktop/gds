use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use gpui_kit::assets::IconName;

use super::{DragPreview, DragSnapshot, transferable_sources};
use crate::entities::file::directory::{
    ContentCategory, ContentType, Entry, EntryDetails, EntryKind, LinkStatus, LinkTarget,
};
use crate::pages::workspace::ui::file_icon::FileIcon;
use gpui_kit::{Bounds, point, px, size};

fn entry(path: &str, kind: EntryKind) -> Entry {
    let path = PathBuf::from(path);
    let name: OsString = path
        .file_name()
        .expect("test paths have a file name")
        .to_owned();
    Entry {
        display_name: name.to_string_lossy().into_owned(),
        path,
        name,
        kind,
        size: None,
        modified: None,
        is_hidden: false,
        details: EntryDetails::default(),
    }
}

fn content(category: ContentCategory) -> ContentType {
    ContentType {
        category,
        format: None,
        mime: None,
    }
}

fn link(status: LinkStatus, kind: EntryKind, content: Option<ContentType>) -> LinkTarget {
    LinkTarget {
        status,
        kind,
        content,
    }
}

fn rows() -> Vec<Entry> {
    vec![
        entry("/work/a.txt", EntryKind::File),
        entry("/work/folder", EntryKind::Directory),
        entry("/work/b.txt", EntryKind::File),
    ]
}

fn row<'a>(rows: &'a [Entry], path: &str) -> &'a Entry {
    rows.iter()
        .find(|entry| entry.path == Path::new(path))
        .expect("test rows contain the requested path")
}

#[test]
fn dragging_a_selected_row_carries_the_whole_selection() {
    let rows = rows();
    let selected = vec![PathBuf::from("/work/a.txt"), PathBuf::from("/work/b.txt")];
    let snapshot = DragSnapshot::new(&rows, &selected);
    let drag = snapshot.drag_for(row(&rows, "/work/a.txt"));

    assert_eq!(drag.paths(), selected);
    assert_eq!(drag.count(), 2);
    assert_eq!(drag.entries[0].name.as_ref(), "a.txt");
    assert_eq!(drag.entries[1].name.as_ref(), "b.txt");
}

#[test]
fn dragging_an_unselected_row_carries_only_that_row() {
    let rows = rows();
    let selected = vec![PathBuf::from("/work/a.txt"), PathBuf::from("/work/b.txt")];
    let snapshot = DragSnapshot::new(&rows, &selected);
    let drag = snapshot.drag_for(row(&rows, "/work/folder"));

    assert_eq!(drag.paths(), vec![PathBuf::from("/work/folder")]);
    assert_eq!(drag.count(), 1);
    assert_eq!(drag.entries[0].icon.base(), IconName::Folder);
}

#[test]
fn name_drag_preserves_the_grab_point_and_clamps_metadata_grabs() {
    let source = Bounds::new(point(px(120.), px(80.)), size(px(180.), px(20.)));
    let row_origin = point(px(100.), px(70.));
    for pointer in [point(px(160.), px(90.)), point(px(420.), px(90.))] {
        let cursor_offset = pointer - row_origin;
        let offset = DragPreview::source_offset(source, cursor_offset, pointer);
        let origin = row_origin + offset;
        let expected = if source.contains(&pointer) {
            source.origin
        } else {
            point(pointer.x - source.size.width, source.origin.y)
        };
        assert_eq!(origin, expected);
        let movement = point(px(30.), px(40.));
        assert_eq!(
            (pointer + movement) - cursor_offset + offset,
            expected + movement
        );
    }
}

#[test]
fn sources_already_in_the_destination_are_not_transferred() {
    let sources = vec![
        PathBuf::from("/work/a.txt"),
        PathBuf::from("/elsewhere/b.txt"),
    ];

    assert_eq!(
        transferable_sources(Path::new("/work"), sources),
        vec![PathBuf::from("/elsewhere/b.txt")]
    );
}

#[test]
fn dropping_a_selection_onto_one_of_its_folders_skips_that_folder() {
    let sources = vec![PathBuf::from("/work/folder"), PathBuf::from("/work/a.txt")];

    assert_eq!(
        transferable_sources(Path::new("/work/folder"), sources),
        vec![PathBuf::from("/work/a.txt")]
    );
}

#[test]
fn a_resolved_link_shows_its_target_category_with_a_link_badge() {
    let mut linked_folder = entry("/work/shortcut", EntryKind::SymbolicLink);
    linked_folder.details.target = Some(link(LinkStatus::Resolved, EntryKind::Directory, None));
    let mut linked_image = entry("/work/cover", EntryKind::SymbolicLink);
    linked_image.details.target = Some(link(
        LinkStatus::Resolved,
        EntryKind::File,
        Some(content(ContentCategory::Image)),
    ));

    let folder_icon = FileIcon::for_entry(&linked_folder);
    assert_eq!(folder_icon.base(), IconName::Folder);
    assert_eq!(folder_icon.badge(), Some(IconName::Link));

    let image_icon = FileIcon::for_entry(&linked_image);
    assert_eq!(image_icon.base(), IconName::FileImage);
    assert_eq!(image_icon.badge(), Some(IconName::Link));
}

#[test]
fn only_a_missing_target_uses_the_broken_link_icon() {
    let mut broken = entry("/work/shortcut", EntryKind::SymbolicLink);
    broken.details.target = Some(link(LinkStatus::Broken, EntryKind::Unknown, None));
    let mut unreachable = entry("/work/mounted", EntryKind::SymbolicLink);
    unreachable.details.target = Some(link(LinkStatus::Unreachable, EntryKind::Unknown, None));

    let broken_icon = FileIcon::for_entry(&broken);
    assert_eq!(broken_icon.base(), IconName::Unlink);
    assert_eq!(broken_icon.badge(), None);

    let unreachable_icon = FileIcon::for_entry(&unreachable);
    assert_ne!(unreachable_icon.base(), IconName::Unlink);
    assert_eq!(unreachable_icon.badge(), Some(IconName::Link));
}

#[test]
fn regular_files_show_their_content_category() {
    let mut image = entry("/work/photo.png", EntryKind::File);
    image.details.content = Some(content(ContentCategory::Image));
    let mut archive = entry("/work/backup.tar", EntryKind::File);
    archive.details.content = Some(content(ContentCategory::Archive));
    let unknown = entry("/work/blob", EntryKind::File);

    assert_eq!(FileIcon::for_entry(&image).base(), IconName::FileImage);
    assert_eq!(FileIcon::for_entry(&archive).base(), IconName::FileArchive);
    assert_eq!(FileIcon::for_entry(&unknown).base(), IconName::File);
    assert_eq!(FileIcon::for_entry(&image).badge(), None);
}
