//! File list, grid tile, and fallback icon/thumbnail artwork rendering.

use gds::service::file_inspection::Request;
use gds::ui::thumbnail::{ThumbnailProps, ViewFn};
use gds::ui::{IconName, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Signal};
use zgui::view::{Either, current_host, install_stylesheet};
use zgui::{css, view};

use crate::entities::file::{directory::EntryKind, ui::FileIcon};
use crate::entities::location::Location;

use super::Inspection;
use crate::pages::workspace::state::model::Row;

const ARTWORK_SHEET: &str = "gds-files-workspace-artwork";

const ARTWORK_CSS: &str = css!(
    r#"
    .gds-files-artwork-slot {
        display: flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        position: relative;
        overflow: visible;
        border-radius: var(--gds-radius-control, 6px);
        color: var(--gds-muted-foreground);
    }

    .gds-files-artwork-slot[data-directory="true"] {
        color: var(--gds-primary);
    }

    .gds-files-artwork-slot[data-selected="true"] {
        color: var(--gds-selection-foreground);
    }

    .gds-files-artwork-slot[data-selected="true"] .gds-files-artwork-slot {
        color: var(--gds-selection-foreground);
    }

    .gds-files-artwork-thumb {
        width: 100%;
        height: 100%;
        overflow: hidden;
        border-radius: var(--gds-radius-control, 6px);
    }

    .gds-files-tile-thumb-wrap {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        color: var(--gds-muted-foreground);
    }

    .gds-files-tile-thumb-wrap[data-directory="true"] {
        color: var(--gds-primary);
    }

    .gds-files-tile-thumb-wrap[data-selected="true"],
    .gds-files-tile-thumb-wrap[data-selected="true"] .gds-files-artwork-slot,
    .gds-files-tile-thumb-wrap[data-selected="true"] .gds-thumbnail__fallback {
        color: var(--gds-selection-foreground);
    }

    .gds-files-tile-thumb.gds-thumbnail {
        width: 100%;
        height: 100%;
        border-radius: var(--gds-radius-control, 6px);
        overflow: hidden;
    }

    .gds-files-tile-thumb-wrap > .gds-files-artwork-badge {
        right: 4px;
        bottom: 4px;
    }

    .gds-files-artwork-badge {
        position: absolute;
        right: -4px;
        bottom: -4px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: transparent;
        color: inherit;
        padding: 0;
        pointer-events: none;
        z-index: 1;
    }
    "#
);

pub(super) fn install_artwork_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(ARTWORK_SHEET, ARTWORK_CSS);
    }
}

pub(super) fn badge_pill_view(badge_icon: IconName, size: f32) -> AnyView {
    let badge_size = (size * 0.55).clamp(10.0, 28.0);
    AnyView::new(view! {
        box(class = "gds-files-artwork-badge", a11y:hidden = true) {
            Icon(name = badge_icon, size = badge_size)
        }
    })
}

pub(super) fn file_artwork_view(icon: FileIcon, size: f32, is_directory: bool) -> AnyView {
    let badge_overlay = icon
        .badge()
        .map(|badge_icon| badge_pill_view(badge_icon, size));
    AnyView::new(view! {
        box(
            class = "gds-files-artwork-slot",
            style:width = format!("{size:.1}px"),
            style:height = format!("{size:.1}px"),
            attr:data-directory = is_directory.then(|| "true".to_owned()),
        ) {
            Icon(name = icon.base(), size = size) {}
            {badge_overlay}
        }
    })
}

impl Inspection<'_> {
    pub(in crate::pages::workspace) fn row_artwork_with_selection(
        &self,
        row: &Row,
        size: f32,
        selected: Signal<f32, LocalStorage>,
    ) -> AnyView {
        self.artwork(row, size, selected)
    }

    pub(in crate::pages::workspace) fn tile_artwork(
        &self,
        row: &Row,
        icon_size: f32,
        selected: Signal<f32, LocalStorage>,
    ) -> AnyView {
        install_artwork_stylesheet();
        let is_directory = match row {
            Row::File(entry) => FileIcon::is_directory(entry),
            Row::Trash(entry) => entry.kind == EntryKind::Directory,
            Row::Remote(entry) => entry.kind == gds::service::remote::EntryKind::Directory,
            Row::Place(Location::Directory(_)) => true,
            _ => false,
        };

        if let Row::File(entry) = row {
            let cache = self.0.0.inspections.get_untracked();
            if let Some(source) = entry.thumbnail_source(&cache) {
                let icon = FileIcon::for_entry(entry);
                let _ = cache.request(&source, Request::Thumbnail);
                let badge_overlay = icon
                    .badge()
                    .map(|badge_icon| badge_pill_view(badge_icon, icon_size));
                let thumb_ref = NodeRef::new();
                #[cfg(test)]
                tests::record_tile_thumb_node(&entry.display_name, thumb_ref);
                return AnyView::new(Either::<_, ()>::Left(view! {
                    box(
                        node_ref = thumb_ref,
                        class = "gds-files-tile-thumb-wrap",
                        attr:data-directory = move || is_directory.then(|| "true".to_owned()),
                        attr:data-selected = move || (selected.get() > 0.5).then(|| "true".to_owned()),
                    ) {
                        Thumbnail(
                            cache = cache,
                            source = source,
                            fallback = ViewFn::new(move || file_artwork_view(icon, icon_size, is_directory)),
                            show_status = false,
                            class = "gds-files-tile-thumb",
                        ) {}
                        {badge_overlay}
                    }
                }));
            }
        }

        let icon = match row {
            Row::File(entry) => FileIcon::for_entry(entry),
            _ => FileIcon::plain(row.icon()),
        };
        let slot_ref = NodeRef::new();
        #[cfg(test)]
        tests::record_tile_fallback_node(&row.label(), slot_ref);
        AnyView::new(Either::<(), _>::Right(view! {
            box(
                node_ref = slot_ref,
                class = "gds-files-artwork-slot",
                style:width = format!("{icon_size:.1}px"),
                style:height = format!("{icon_size:.1}px"),
                attr:data-directory = move || is_directory.then(|| "true".to_owned()),
                attr:data-selected = move || (selected.get() > 0.5).then(|| "true".to_owned()),
            ) {
                {file_artwork_view(icon, icon_size, is_directory)}
            }
        }))
    }

    pub(in crate::pages::workspace) fn artwork(
        &self,
        row: &Row,
        size: f32,
        selected: Signal<f32, LocalStorage>,
    ) -> AnyView {
        install_artwork_stylesheet();
        let is_directory = match row {
            Row::File(entry) => FileIcon::is_directory(entry),
            Row::Trash(entry) => entry.kind == EntryKind::Directory,
            Row::Remote(entry) => entry.kind == gds::service::remote::EntryKind::Directory,
            Row::Place(Location::Directory(_)) => true,
            _ => false,
        };
        let inner = match row {
            Row::File(entry) => {
                let icon = FileIcon::for_entry(entry);
                let cache = self.0.0.inspections.get_untracked();
                if size >= 32.0
                    && let Some(source) = entry.thumbnail_source(&cache)
                {
                    let _ = cache.request(&source, Request::Thumbnail);
                    let badge_overlay = icon
                        .badge()
                        .map(|badge_icon| badge_pill_view(badge_icon, size));
                    AnyView::new(Either::<_, ()>::Left(view! {
                        box(
                            class = "gds-files-artwork-slot",
                            style:width = format!("{size:.1}px"),
                            style:height = format!("{size:.1}px"),
                            attr:data-directory = move || is_directory.then(|| "true".to_owned()),
                        ) {
                            box(class = "gds-files-artwork-thumb") {
                                Thumbnail(
                                    cache = cache,
                                    source = source,
                                    fallback = ViewFn::new(move || file_artwork_view(icon, size, is_directory)),
                                    show_status = false,
                                )
                            }
                            {badge_overlay}
                        }
                    }))
                } else {
                    file_artwork_view(icon, size, is_directory)
                }
            }
            Row::Trash(_) => file_artwork_view(FileIcon::plain(row.icon()), size, is_directory),
            _ => file_artwork_view(FileIcon::plain(row.icon()), size, is_directory),
        };

        AnyView::new(view! {
            box(
                class = "gds-files-artwork-slot",
                style:width = format!("{size:.1}px"),
                style:height = format!("{size:.1}px"),
                attr:data-directory = is_directory.then(|| "true".to_owned()),
                attr:data-selected = move || (selected.get() > 0.5).then(|| "true".to_owned()),
            ) {
                {inner}
            }
        })
    }
}

#[cfg(test)]
pub(in crate::pages::workspace) mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use zgui::prelude::NodeRef;

    use super::super::details::format_two_line_title;

    thread_local! {
        static TILE_THUMB_NODES: RefCell<HashMap<String, NodeRef>> = RefCell::new(HashMap::new());
        static TILE_FALLBACK_NODES: RefCell<HashMap<String, NodeRef>> = RefCell::new(HashMap::new());
        static DETAILS_THUMB_NODE: RefCell<Option<NodeRef>> = const { RefCell::new(None) };
        static DETAILS_FALLBACK_NODE: RefCell<Option<NodeRef>> = const { RefCell::new(None) };
        static DETAILS_TITLE_SLOT_NODE: RefCell<Option<NodeRef>> = const { RefCell::new(None) };
        static DETAILS_BODY_NODE: RefCell<Option<NodeRef>> = const { RefCell::new(None) };
    }

    pub(super) fn record_tile_thumb_node(name: &str, node: NodeRef) {
        TILE_THUMB_NODES.with(|slot| {
            slot.borrow_mut().insert(name.to_owned(), node);
        });
    }

    pub(super) fn record_tile_fallback_node(name: &str, node: NodeRef) {
        TILE_FALLBACK_NODES.with(|slot| {
            slot.borrow_mut().insert(name.to_owned(), node);
        });
    }

    pub(in crate::pages::workspace::panes::inspection) fn record_details_thumb_node(node: NodeRef) {
        DETAILS_THUMB_NODE.with(|slot| {
            *slot.borrow_mut() = Some(node);
        });
        DETAILS_FALLBACK_NODE.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }

    pub(in crate::pages::workspace::panes::inspection) fn record_details_fallback_node(
        node: NodeRef,
    ) {
        DETAILS_FALLBACK_NODE.with(|slot| {
            *slot.borrow_mut() = Some(node);
        });
    }

    pub(in crate::pages::workspace::panes::inspection) fn record_details_title_slot_node(
        node: NodeRef,
    ) {
        DETAILS_TITLE_SLOT_NODE.with(|slot| {
            *slot.borrow_mut() = Some(node);
        });
    }

    pub(in crate::pages::workspace::panes::inspection) fn record_details_body_node(node: NodeRef) {
        DETAILS_BODY_NODE.with(|slot| {
            *slot.borrow_mut() = Some(node);
        });
    }

    pub(in crate::pages::workspace) fn clear_recorded_artwork_nodes() {
        TILE_THUMB_NODES.with(|slot| slot.borrow_mut().clear());
        TILE_FALLBACK_NODES.with(|slot| slot.borrow_mut().clear());
        DETAILS_THUMB_NODE.with(|slot| *slot.borrow_mut() = None);
        DETAILS_FALLBACK_NODE.with(|slot| *slot.borrow_mut() = None);
        DETAILS_TITLE_SLOT_NODE.with(|slot| *slot.borrow_mut() = None);
        DETAILS_BODY_NODE.with(|slot| *slot.borrow_mut() = None);
    }

    pub(in crate::pages::workspace) fn tile_thumb_node(name: &str) -> Option<NodeRef> {
        TILE_THUMB_NODES.with(|slot| slot.borrow().get(name).copied())
    }

    pub(in crate::pages::workspace) fn tile_fallback_node(name: &str) -> Option<NodeRef> {
        TILE_FALLBACK_NODES.with(|slot| slot.borrow().get(name).copied())
    }

    pub(in crate::pages::workspace) fn details_thumb_node() -> Option<NodeRef> {
        DETAILS_THUMB_NODE.with(|slot| *slot.borrow())
    }

    pub(in crate::pages::workspace) fn details_fallback_node() -> Option<NodeRef> {
        DETAILS_FALLBACK_NODE.with(|slot| *slot.borrow())
    }

    pub(in crate::pages::workspace) fn details_title_slot_node() -> Option<NodeRef> {
        DETAILS_TITLE_SLOT_NODE.with(|slot| *slot.borrow())
    }

    pub(in crate::pages::workspace) fn details_body_node() -> Option<NodeRef> {
        DETAILS_BODY_NODE.with(|slot| *slot.borrow())
    }

    #[test]
    fn two_line_title_preserves_full_two_line_names_and_middle_truncates_longer_names_keeping_extension()
     {
        let (short, tip_short) = format_two_line_title("notes.txt", 48);
        assert_eq!(short, "notes.txt");
        assert!(!tip_short);

        let hash_name = "eb72d0c7423da74ca3d41fb9e570ce33470697977.png";
        let (two_line, tip_two_line) = format_two_line_title(hash_name, 48);
        assert_eq!(two_line, hash_name, "45-char 2-line filename stays intact");
        assert!(tip_two_line);

        let very_long =
            "release_artifact_2026_11_21_x86_64_unknown_linux_gnu_symbols_bundle.tar.zst";
        let (clamped, tip_long) = format_two_line_title(very_long, 48);
        assert!(tip_long);
        assert!(clamped.contains('…'));
        assert!(
            clamped.ends_with(".tar.zst"),
            "middle truncation must preserve trailing compound extension, got {clamped:?}"
        );
        assert!(unicode_width::UnicodeWidthStr::width(clamped.as_str()) <= 48);
    }
}
