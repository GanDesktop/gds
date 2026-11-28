//! Workspace details sidebar pane, standalone properties window, and shared details layout.

#[cfg(unix)]
use std::collections::HashSet;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use gds::app::Gds;
use gds::service::file_inspection::{FileFacts, Metadata, Request, Source as InspectionSource};
use gds::service::file_tags;
use gds::service::task_manager::Scope;
use gds::ui::button::{ButtonProps, ButtonVariant};
use gds::ui::capacity::CapacityBarProps;
use gds::ui::feedback::tooltip::HintTooltipProps;
use gds::ui::file_inspection::{Cache, Options, State as InspectionState};
use gds::ui::frame::FrameProps;
use gds::ui::thumbnail::{ThumbnailProps, ViewFn};
use gds::ui::time::Timestamp;
use gds::ui::window::{TitleBarProps, workspace_options};
use gds::ui::{IconName, IconProps, Shortcut};
use gds::{i18n, t};
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, on_cleanup_local, spawn_local};
use zgui::runtime::{WindowHandle, try_use_windows};
use zgui::view::{Either, EventCx, current_host, events, install_stylesheet};
use zgui::vocab::{Key, Modifiers, NamedKey};
use zgui::{css, view};
use zgui_ui::SpinnerProps;

use crate::entities::file::{
    directory::{self, Entry, EntryKind},
    ui::{EntryPresentation, FileIcon, UNKNOWN_VALUE},
};
use crate::entities::location::{Location, volumes};
use crate::entities::preferences;

use super::Inspection;
use super::artwork::{badge_pill_view, file_artwork_view, install_artwork_stylesheet};
use super::info;
use crate::pages::workspace::{
    projection::Geometry,
    state::{
        context::Context,
        model::{Contents, Row},
    },
};

const DETAILS_SHEET: &str = "gds-files-workspace-details";

const DETAILS_CSS: &str = css!(
    r#"
    .gds-files-details {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        box-sizing: border-box;
        background: transparent;
        border-left: 1px solid var(--gds-border);
    }

    .gds-files-details__scroll {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0;
        min-height: 0;
        overflow-y: auto;
        overflow-x: hidden;
    }

    .gds-files-details__empty {
        flex: 1 1 0%;
        align-items: center;
        justify-content: center;
        gap: 10px;
        padding: 20px;
        background: transparent;
        border-left: 1px solid var(--gds-border);
    }

    .gds-files-details__empty-icon {
        width: 28px;
        height: 28px;
        color: var(--gds-muted-foreground);
    }

    .gds-files-details__empty-text {
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-muted-foreground);
        text-align: center;
    }

    .gds-files-details__content {
        display: flex;
        flex-direction: column;
        flex: 0 0 auto;
        width: 100%;
        min-width: 0;
    }

    .gds-files-details__hero {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0;
        flex-shrink: 0;
        align-items: center;
        gap: 10px;
        padding: 14px 14px 12px 14px;
        box-sizing: border-box;
        border-bottom: 1px solid var(--gds-border);
    }

    .gds-files-details__body {
        display: flex;
        flex-direction: column;
        flex: 0 0 auto;
        width: 100%;
        min-width: 0;
        padding: 12px 14px 14px 14px;
        gap: 12px;
        box-sizing: border-box;
    }

    .gds-files-details__fallback-slot {
        width: 64px;
        height: 64px;
        flex-shrink: 0;
    }

    .gds-files-details__thumb-wrap {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        height: 168px;
        border-radius: 8px;
        border: 1px solid var(--gds-border);
        background: var(--gds-surface-container-low, var(--gds-surface));
        box-sizing: border-box;
        overflow: hidden;
        flex-shrink: 0;
        color: var(--gds-muted-foreground);
    }

    .gds-files-details__thumb.gds-thumbnail {
        width: 100%;
        height: 100%;
    }

    .gds-files-details__thumb-wrap > .gds-files-artwork-badge {
        right: 6px;
        bottom: 6px;
    }

    .gds-files-details__title-slot {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        min-width: 0;
        height: calc(44px * var(--gds-font-scale, 1));
        flex-shrink: 0;
        overflow: hidden;
    }

    .gds-files-details__title-trigger.zui-tooltip__trigger {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        max-width: 100%;
        min-width: 0;
        max-height: 100%;
        overflow: hidden;
    }

    .gds-files-details__title {
        display: block;
        width: 100%;
        min-width: 0;
        max-height: calc(44px * var(--gds-font-scale, 1));
        font-size: var(--gds-text-md, 16px);
        line-height: calc(22px * var(--gds-font-scale, 1));
        font-weight: 600;
        color: var(--gds-foreground);
        text-align: center;
        white-space: normal;
        overflow-wrap: anywhere;
        overflow: hidden;
    }

    .gds-files-details__rows {
        gap: 8px;
    }

    .gds-files-details__row {
        display: flex;
        flex-direction: row;
        align-items: baseline;
        justify-content: space-between;
        gap: 12px;
    }

    .gds-files-details__label {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        flex-shrink: 0;
    }

    .gds-files-details__value {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-foreground);
        text-align: right;
        min-width: 0;
        overflow-wrap: anywhere;
    }

    .gds-files-details__tags-section {
        gap: 8px;
        padding-top: 10px;
        border-top: 1px solid var(--gds-border);
    }

    .gds-files-details__comment-text {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-foreground);
        text-align: left;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
    }

    .gds-files-details__tags-list {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        gap: 6px;
    }

    .gds-files-details__tag-chip {
        padding: 2px 8px;
        border-radius: 999px;
        font-size: var(--gds-text-xs, 12px);
        background: var(--gds-surface);
        border: 1px solid var(--gds-border);
        color: var(--gds-foreground);
    }

    .gds-files-properties-root {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
    }

    .gds-files-properties-root:focus {
        outline: none;
    }

    .gds-files-properties-window {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background-color: var(--gds-content-bg, var(--gds-content));
    }
    "#
);

fn install_details_stylesheet() {
    install_artwork_stylesheet();
    if current_host().is_some() {
        install_stylesheet(DETAILS_SHEET, DETAILS_CSS);
    }
}

fn metadata_row(label_key: &'static str, value: AnyView) -> AnyView {
    AnyView::new(view! {
        row(class = "gds-files-details__row") {
            text(class = "gds-files-details__label") {{i18n::text(label_key)}}
            box(class = "gds-files-details__value") {
                {value}
            }
        }
    })
}

fn text_row(label_key: &'static str, value: String) -> AnyView {
    metadata_row(
        label_key,
        AnyView::new(view! {
            text(class = "gds-files-details__value") {{value}}
        }),
    )
}

/// Format a title for the fixed 2-line details slot (`max_cols` display width units across two
/// lines), preserving the trailing extension/suffix via middle ellipsis when clamping 3+ line
/// names and reporting whether a hover tooltip should expose the full unabbreviated title.
pub(super) fn format_two_line_title(name: &str, max_cols: usize) -> (String, bool) {
    let total = name.width();
    if total <= max_cols {
        return (name.to_owned(), total > max_cols.saturating_sub(4));
    }
    let budget = max_cols.saturating_sub(1);
    let tail_budget = (budget / 3).clamp(8, 16);
    let mut tail_parts: Vec<&str> = Vec::new();
    let mut tail_cols = 0usize;
    for grapheme in name.graphemes(true).rev() {
        let w = grapheme.width();
        if tail_cols + w > tail_budget {
            break;
        }
        tail_cols += w;
        tail_parts.push(grapheme);
    }
    tail_parts.reverse();

    let head_budget = budget.saturating_sub(tail_cols);
    let mut out = String::with_capacity(name.len());
    let mut head_cols = 0usize;
    for grapheme in name.graphemes(true) {
        let w = grapheme.width();
        if head_cols + w > head_budget {
            break;
        }
        head_cols += w;
        out.push_str(grapheme);
    }
    out.push('…');
    for part in tail_parts {
        out.push_str(part);
    }
    (out, true)
}

fn details_stage_media_view(
    cache: Cache,
    thumb_src: InspectionSource,
    icon: FileIcon,
    is_directory: bool,
) -> AnyView {
    let badge_overlay = icon
        .badge()
        .map(|badge_icon| badge_pill_view(badge_icon, 36.0));
    let thumb_wrap_ref = NodeRef::new();
    #[cfg(test)]
    super::artwork::tests::record_details_thumb_node(thumb_wrap_ref);
    AnyView::new(view! {
        box(node_ref = thumb_wrap_ref, class = "gds-files-details__thumb-wrap") {
            {Either::<_, ()>::Left(view! {
                Thumbnail(
                    cache = cache,
                    source = thumb_src,
                    fallback = ViewFn::new(move || file_artwork_view(icon, 64.0, is_directory)),
                    show_status = true,
                    class = "gds-files-details__thumb",
                ) {}
                {badge_overlay}
            })}
        }
    })
}

fn details_stage_icon_view(icon: FileIcon, is_directory: bool) -> AnyView {
    let thumb_wrap_ref = NodeRef::new();
    let fallback_ref = NodeRef::new();
    #[cfg(test)]
    {
        super::artwork::tests::record_details_thumb_node(thumb_wrap_ref);
        super::artwork::tests::record_details_fallback_node(fallback_ref);
    }
    let badge_overlay = icon
        .badge()
        .map(|badge_icon| badge_pill_view(badge_icon, 64.0));
    AnyView::new(view! {
        box(node_ref = thumb_wrap_ref, class = "gds-files-details__thumb-wrap") {
            {Either::<(), _>::Right(view! {
                box(
                    node_ref = fallback_ref,
                    class = "gds-files-artwork-slot gds-files-details__fallback-slot",
                    attr:data-directory = move || is_directory.then(|| "true".to_owned()),
                ) {
                    Icon(name = icon.base(), size = 64.0) {}
                    {badge_overlay}
                }
            })}
        }
    })
}

fn details_hero_view(stage: AnyView, full_title: &str, panel_chrome: bool) -> AnyView {
    let max_cols = if panel_chrome { 48 } else { 72 };
    let (display_title, show_tooltip) = format_two_line_title(full_title, max_cols);
    let title_slot_ref = NodeRef::new();
    #[cfg(test)]
    super::artwork::tests::record_details_title_slot_node(title_slot_ref);
    let inner = if show_tooltip {
        let tip = full_title.to_owned();
        Either::Left(
            view! {
                HintTooltip(text = tip, trigger_class = "gds-files-details__title-trigger") {
                    text(class = "gds-files-details__title") {{display_title}}
                }
            }
            .into_view(),
        )
    } else {
        Either::Right(
            view! {
                text(class = "gds-files-details__title") {{display_title}}
            }
            .into_view(),
        )
    };
    AnyView::new(view! {
        column(class = "gds-files-details__hero") {
            {stage}
            box(node_ref = title_slot_ref, class = "gds-files-details__title-slot") {
                {inner}
            }
        }
    })
}

fn details_content_view(hero: AnyView, body: AnyView) -> AnyView {
    let body_ref = NodeRef::new();
    #[cfg(test)]
    super::artwork::tests::record_details_body_node(body_ref);
    AnyView::new(view! {
        column(class = "gds-files-details__scroll") {
            column(class = "gds-files-details__content") {
                {hero}
                column(node_ref = body_ref, class = "gds-files-details__body") {
                    {body}
                }
            }
        }
    })
}

const MULTI_DIR_ENTRY_LIMIT: usize = 200_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MultiDirSizeState {
    Loading,
    Ready { bytes: u64, capped: bool },
}

struct MultiDirSizeJob {
    dirs: Vec<PathBuf>,
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
}

impl MultiDirSizeJob {
    fn new() -> Self {
        Self {
            dirs: Vec::new(),
            generation: 0,
            cancel: None,
        }
    }

    fn cancel_active(&mut self) {
        if let Some(flag) = self.cancel.take() {
            flag.store(true, Ordering::Relaxed);
        }
        self.dirs.clear();
    }
}

fn measure_local_directories(dirs: &[PathBuf], cancel: &AtomicBool) -> Option<(u64, bool)> {
    let mut total = 0u64;
    let mut visited = 0usize;
    let mut capped = false;
    #[cfg(unix)]
    let mut seen_hardlinks: HashSet<(u64, u64)> = HashSet::new();

    for root in dirs {
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            if visited.is_multiple_of(64) && cancel.load(Ordering::Relaxed) {
                return None;
            }
            visited += 1;
            if visited > MULTI_DIR_ENTRY_LIMIT {
                capped = true;
                return Some((total, capped));
            }
            let Ok(entry) = entry else {
                continue;
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt as _;
                if meta.nlink() > 1 && !seen_hardlinks.insert((meta.dev(), meta.ino())) {
                    continue;
                }
            }
            total = total.saturating_add(meta.len());
        }
    }
    (!cancel.load(Ordering::Relaxed)).then_some((total, capped))
}

fn empty_details_state(icon: IconName, text: String) -> AnyView {
    AnyView::new(view! {
        column(class = "gds-files-details__empty") {
            Icon(name = icon, class = "gds-files-details__empty-icon")
            text(class = "gds-files-details__empty-text") {{text}}
        }
    })
}

fn static_row_details(ctx: &Context, row: &Row) -> AnyView {
    match row {
        Row::Place(location) => {
            let hero = details_hero_view(
                details_stage_icon_view(
                    FileIcon::plain(location.icon()),
                    matches!(location, Location::Directory(_)),
                ),
                &location.label(),
                true,
            );
            let body = AnyView::new(view! {
                column(class = "gds-files-details__rows") {
                    {text_row("place-location", location.display_address())}
                }
            });
            AnyView::new(view! {
                column(class = "gds-files-details") {
                    {details_content_view(hero, body)}
                }
            })
        }
        Row::Trash(entry) => {
            let size = entry
                .size
                .map(EntryPresentation::format_size)
                .or_else(|| entry.items.map(|n| i18n::count("item-count", n)))
                .unwrap_or_else(|| UNKNOWN_VALUE.to_owned());
            let is_dir = entry.kind == EntryKind::Directory;
            let title = row.label();
            let hero = details_hero_view(
                details_stage_icon_view(FileIcon::plain(row.icon()), is_dir),
                &title,
                true,
            );
            let body = AnyView::new(view! {
                column(class = "gds-files-details__rows") {
                    {text_row("recycle-original-location", entry.original_path.display().to_string())}
                    {metadata_row("recycle-deleted-at", AnyView::new(view! { {Timestamp::new(entry.deleted_at)} }))}
                    {text_row("size", size)}
                }
            });
            AnyView::new(view! {
                column(class = "gds-files-details") {
                    {details_content_view(hero, body)}
                }
            })
        }
        Row::Tag(tag) => {
            let title = row.label();
            let hero = details_hero_view(
                details_stage_icon_view(FileIcon::plain(IconName::Tag), false),
                &title,
                true,
            );
            let body = AnyView::new(view! {
                column(class = "gds-files-details__rows") {
                    {text_row("tags-title", tag.clone())}
                }
            });
            AnyView::new(view! {
                column(class = "gds-files-details") {
                    {details_content_view(hero, body)}
                }
            })
        }
        Row::Remote(entry) => {
            let size = entry
                .size
                .map(EntryPresentation::format_size)
                .unwrap_or_else(|| UNKNOWN_VALUE.to_owned());
            let is_dir = entry.kind == gds::service::remote::EntryKind::Directory;
            let hero = details_hero_view(
                details_stage_icon_view(FileIcon::plain(row.icon()), is_dir),
                &entry.name,
                true,
            );
            let body = AnyView::new(view! {
                column(class = "gds-files-details__rows") {
                    {text_row("place-location", entry.address.uri().to_owned())}
                    {metadata_row("modified", AnyView::new(view! { {Timestamp::new(entry.modified)} }))}
                    {text_row("size", size)}
                }
            });
            AnyView::new(view! {
                column(class = "gds-files-details") {
                    {details_content_view(hero, body)}
                }
            })
        }
        Row::Volume(volume) => {
            let state = ctx.0.volumes.capacity(&volume.id);
            let (cap_label, fraction, unavailable) = match state {
                Some(volumes::CapacityState::Ready(cap)) => (
                    t!(
                        "device-capacity",
                        available = EntryPresentation::format_size(cap.available),
                        total = EntryPresentation::format_size(cap.total)
                    ),
                    Some(cap.unavailable_fraction()),
                    false,
                ),
                Some(volumes::CapacityState::Unavailable(reason)) => (reason, None, true),
                None | Some(volumes::CapacityState::Loading) => {
                    if volume.mount_points.is_empty() {
                        (i18n::text("device-not-mounted"), None, true)
                    } else {
                        (i18n::text("device-capacity-loading"), None, false)
                    }
                }
            };
            let fs = if volume.filesystem.is_empty() {
                UNKNOWN_VALUE.to_owned()
            } else {
                volume.filesystem.clone()
            };
            let mounts = if volume.mount_points.is_empty() {
                i18n::text("device-not-mounted")
            } else {
                volume
                    .mount_points
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let hero = details_hero_view(
                details_stage_icon_view(FileIcon::plain(IconName::HardDrive), false),
                &volume.name,
                true,
            );
            let body = AnyView::new(view! {
                column(class = "gds-files-details__rows") {
                    {text_row("volume-format", fs)}
                    {text_row("volume-default-mount", mounts)}
                }
                CapacityBar(label = cap_label, fraction = fraction, unavailable = unavailable)
            });
            AnyView::new(view! {
                column(class = "gds-files-details") {
                    {details_content_view(hero, body)}
                }
            })
        }
        Row::File(_) => AnyView::new(()),
    }
}

impl Inspection<'_> {
    pub(in crate::pages::workspace) fn toggle_details(&self) {
        let ctx = self.0;
        if !Geometry(ctx).details_permitted_untracked() {
            return;
        }
        ctx.0.details.update(|visible| *visible = !*visible);
        let visible = ctx.0.details.get_untracked();
        let location = ctx.0.state.borrow().tab().navigation.location().clone();
        ctx.update(|state| {
            state.places.set_details(location, visible);
        });
        ctx.save_places();
    }

    pub(in crate::pages::workspace) fn details_view(&self) -> AnyView {
        install_details_stylesheet();
        let ctx = self.0.clone();
        let cached_entry: Rc<
            RefCell<Option<(PathBuf, Option<(i64, i64)>, bool, u64, Properties)>>,
        > = Rc::new(RefCell::new(None));
        let details_rev = self.0.0.details_revision;
        let multi_dir_job = Rc::new(RefCell::new(MultiDirSizeJob::new()));
        let multi_dir_sig = self.0.0.owner.with(|| {
            let job_cleanup = Rc::clone(&multi_dir_job);
            on_cleanup_local(move || job_cleanup.borrow_mut().cancel_active());
            RwSignal::new_local(MultiDirSizeState::Loading)
        });

        AnyView::new(move || {
            let (pure_tags, selected) = ctx.read(|state| {
                let tab = state.tab();
                let pure_tags = matches!(tab.contents, Contents::Tags { .. })
                    && !tab.rows.iter().any(|row| matches!(row, Row::File(_)));
                (pure_tags, tab.selected())
            });

            if pure_tags {
                multi_dir_job.borrow_mut().cancel_active();
                return AnyView::new(());
            }

            if selected.is_empty() {
                multi_dir_job.borrow_mut().cancel_active();
                return empty_details_state(IconName::Info, i18n::text("preview-empty"));
            }

            if selected.len() > 1 {
                let count = selected.len();
                let mut file_bytes = 0u64;
                let mut folders = 0usize;
                let mut files = 0usize;
                let mut links = 0usize;
                let mut local_dirs: Vec<PathBuf> = Vec::new();
                let mut has_unsized_non_local = false;

                for item in &selected {
                    match item {
                        Row::File(e) => match e.kind {
                            EntryKind::Directory => {
                                folders += 1;
                                local_dirs.push(e.path.clone());
                            }
                            EntryKind::SymbolicLink => {
                                links += 1;
                            }
                            EntryKind::File | EntryKind::Other => {
                                files += 1;
                                if let Some(bytes) = e.size {
                                    file_bytes = file_bytes.saturating_add(bytes);
                                } else {
                                    has_unsized_non_local = true;
                                }
                            }
                            EntryKind::Unknown => {
                                has_unsized_non_local = true;
                            }
                        },
                        Row::Trash(e) => match e.kind {
                            EntryKind::Directory => {
                                folders += 1;
                                has_unsized_non_local = true;
                            }
                            EntryKind::SymbolicLink => {
                                links += 1;
                            }
                            EntryKind::File | EntryKind::Other => {
                                files += 1;
                                if let Some(bytes) = e.size {
                                    file_bytes = file_bytes.saturating_add(bytes);
                                } else {
                                    has_unsized_non_local = true;
                                }
                            }
                            EntryKind::Unknown => {
                                has_unsized_non_local = true;
                            }
                        },
                        Row::Remote(e) => match e.kind {
                            gds::service::remote::EntryKind::Directory => {
                                folders += 1;
                                has_unsized_non_local = true;
                            }
                            gds::service::remote::EntryKind::SymbolicLink => {
                                links += 1;
                            }
                            gds::service::remote::EntryKind::File
                            | gds::service::remote::EntryKind::Other => {
                                files += 1;
                                if let Some(bytes) = e.size {
                                    file_bytes = file_bytes.saturating_add(bytes);
                                } else {
                                    has_unsized_non_local = true;
                                }
                            }
                        },
                        Row::Place(Location::Directory(path)) => {
                            folders += 1;
                            local_dirs.push(PathBuf::from(path));
                        }
                        Row::Place(_) | Row::Tag(_) | Row::Volume(_) => {
                            has_unsized_non_local = true;
                        }
                    }
                }

                let size_row = if !local_dirs.is_empty() {
                    let needs_spawn = multi_dir_job.borrow().dirs != local_dirs;
                    if needs_spawn {
                        let cancel = Arc::new(AtomicBool::new(false));
                        let my_gen = {
                            let mut job = multi_dir_job.borrow_mut();
                            if let Some(prev) = job.cancel.take() {
                                prev.store(true, Ordering::Relaxed);
                            }
                            job.generation = job.generation.wrapping_add(1);
                            job.dirs.clone_from(&local_dirs);
                            job.cancel = Some(Arc::clone(&cancel));
                            job.generation
                        };
                        let _ = multi_dir_sig.try_set(MultiDirSizeState::Loading);
                        let dirs_for_task = local_dirs;
                        let cancel_for_task = Arc::clone(&cancel);
                        let job_ref = Rc::clone(&multi_dir_job);
                        let sig = multi_dir_sig;
                        let task = ctx.scope().spawn_blocking_named(
                            "Calculate selected directory sizes",
                            move || measure_local_directories(&dirs_for_task, &cancel_for_task),
                        );
                        drop(spawn_local(async move {
                            if let Ok(Some((bytes, capped))) = task.await
                                && job_ref.borrow().generation == my_gen
                            {
                                let _ = sig.try_set(MultiDirSizeState::Ready { bytes, capped });
                            }
                        }));
                    }
                    metadata_row(
                        "size",
                        AnyView::new(view! {
                            text(class = "gds-files-details__value") {
                                {move || match multi_dir_sig.get() {
                                    MultiDirSizeState::Loading => i18n::text("details-calculating"),
                                    MultiDirSizeState::Ready { bytes, capped } => {
                                        let sum = file_bytes.saturating_add(bytes);
                                        let formatted = EntryPresentation::format_size(sum);
                                        if capped || has_unsized_non_local {
                                            format!("≥ {formatted}")
                                        } else {
                                            formatted
                                        }
                                    }
                                }}
                            }
                        }),
                    )
                } else {
                    multi_dir_job.borrow_mut().cancel_active();
                    let size_label = if files > 0 && !has_unsized_non_local {
                        EntryPresentation::format_size(file_bytes)
                    } else if file_bytes > 0 {
                        format!("≥ {}", EntryPresentation::format_size(file_bytes))
                    } else if files == 0 && links > 0 && !has_unsized_non_local {
                        EntryPresentation::format_size(0)
                    } else {
                        UNKNOWN_VALUE.to_owned()
                    };
                    text_row("size", size_label)
                };

                let mut rows = vec![text_row("details-items", count.to_string()), size_row];
                if folders > 0 {
                    rows.push(text_row("details-folders", folders.to_string()));
                }
                if files > 0 {
                    rows.push(text_row("details-files", files.to_string()));
                }
                if links > 0 {
                    rows.push(text_row("details-links", links.to_string()));
                }

                let summary_title = t!("preview-multiple", count);
                let hero = details_hero_view(
                    details_stage_icon_view(FileIcon::multiple(), false),
                    &summary_title,
                    true,
                );
                let body = AnyView::new(view! {
                    column(class = "gds-files-details__rows") {
                        {rows}
                    }
                });
                return AnyView::new(view! {
                    column(class = "gds-files-details") {
                        {details_content_view(hero, body)}
                    }
                });
            }

            multi_dir_job.borrow_mut().cancel_active();

            let row = &selected[0];
            let tags_enabled = ctx.tags_enabled();
            let rev = details_rev.get();
            let local_entry_or_place: Option<Entry> = match row {
                Row::File(entry) => Some(entry.clone()),
                Row::Place(Location::Directory(path)) => {
                    let path_buf = PathBuf::from(path);
                    let props = {
                        let cache_hit = cached_entry
                            .borrow()
                            .as_ref()
                            .filter(|(p, changed, enabled, cached_rev, _)| {
                                *p == path_buf
                                    && changed.is_none()
                                    && *enabled == tags_enabled
                                    && *cached_rev == rev
                            })
                            .map(|(_, _, _, _, props)| props.clone());
                        if let Some(hit) = cache_hit {
                            hit
                        } else {
                            // CRITICAL (#94): Allocate signals on `ctx.0.owner`, NEVER inside
                            // this transient reactive closure, so cached `Properties` survive
                            // preference/selection re-runs without disposed-signal panics.
                            let scope = ctx.scope();
                            let created = ctx.0.owner.with(|| {
                                Properties::new_with_tags(path_buf.clone(), scope, tags_enabled)
                            });
                            *cached_entry.borrow_mut() =
                                Some((path_buf, None, tags_enabled, rev, created.clone()));
                            created
                        }
                    };
                    return props.view(true);
                }
                _ => None,
            };

            if let Some(entry) = local_entry_or_place {
                let props = {
                    let cache_hit = cached_entry
                        .borrow()
                        .as_ref()
                        .filter(|(p, changed, enabled, cached_rev, _)| {
                            *p == entry.path
                                && *changed == entry.details.changed
                                && *enabled == tags_enabled
                                && *cached_rev == rev
                        })
                        .map(|(_, _, _, _, props)| props.clone());
                    if let Some(hit) = cache_hit {
                        hit
                    } else {
                        let cache = ctx.0.inspections.get_untracked();
                        let scope = ctx.scope();
                        let entry_for_props = entry.clone();
                        let created = ctx.0.owner.with(|| {
                            Properties::from_entry(entry_for_props, cache, scope, tags_enabled)
                        });
                        *cached_entry.borrow_mut() = Some((
                            entry.path.clone(),
                            entry.details.changed,
                            tags_enabled,
                            rev,
                            created.clone(),
                        ));
                        created
                    }
                };
                return props.view(true);
            }

            static_row_details(&ctx, row)
        })
    }
}

thread_local! {
    static PROPERTIES_WINDOWS: RefCell<HashMap<PathBuf, WindowHandle>> = RefCell::new(HashMap::new());
}

#[derive(Clone)]
enum EntryState {
    Loading,
    Ready(Entry),
    Failed(String),
}

#[derive(Clone)]
struct PropertiesInner {
    path: PathBuf,
    scope: Scope,
    cache: Cache,
    entry: RwSignal<EntryState, LocalStorage>,
    tags: RwSignal<Option<Result<Vec<String>, String>>, LocalStorage>,
    origin_url: RwSignal<Option<String>, LocalStorage>,
    comment: RwSignal<Option<Result<Option<String>, String>>, LocalStorage>,
    description_version: RwSignal<u64, LocalStorage>,
    tags_enabled: bool,
}

#[derive(Clone)]
pub(crate) struct Properties(Rc<PropertiesInner>);

impl Properties {
    pub(crate) fn new(path: PathBuf, scope: Scope) -> Self {
        let tags_enabled = preferences::Service::current().snapshot().tags.enabled;
        Self::new_with_tags(path, scope, tags_enabled)
    }

    fn new_with_tags(path: PathBuf, scope: Scope, tags_enabled: bool) -> Self {
        let cache = Cache::with_scope(scope.clone(), Options::default());
        let inner = Rc::new(PropertiesInner {
            path,
            scope,
            cache,
            entry: RwSignal::new_local(EntryState::Loading),
            tags: RwSignal::new_local(None),
            origin_url: RwSignal::new_local(None),
            comment: RwSignal::new_local(None),
            description_version: RwSignal::new_local(0),
            tags_enabled,
        });
        let props = Self(inner);
        props.reload();
        props
    }

    fn from_entry(entry: Entry, cache: Cache, scope: Scope, tags_enabled: bool) -> Self {
        let path = entry.path.clone();
        let is_file = entry.kind == EntryKind::File;
        let is_symlink = entry.kind == EntryKind::SymbolicLink;
        let missing_mime = entry
            .details
            .display_content()
            .and_then(|content| content.mime.as_ref())
            .filter(|mime| !directory::mime_descriptions::is_cached(mime))
            .cloned();
        let inner = Rc::new(PropertiesInner {
            path,
            scope,
            cache,
            entry: RwSignal::new_local(EntryState::Ready(entry)),
            tags: RwSignal::new_local(None),
            origin_url: RwSignal::new_local(None),
            comment: RwSignal::new_local(None),
            description_version: RwSignal::new_local(0),
            tags_enabled,
        });
        let props = Self(inner);
        if let Some(mime) = missing_mime {
            props.load_description(mime);
        }
        if is_file {
            props.load_origin_url();
        }
        if cfg!(target_os = "linux") && !is_symlink {
            props.load_comment();
        }
        if tags_enabled {
            props.load_tags();
        }
        props
    }

    #[cfg(feature = "files-e2e")]
    pub(crate) fn open(path: PathBuf, scope: Scope) {
        Self::open_with_activation(path, scope, None);
    }

    pub(crate) fn open_with_activation(
        path: PathBuf,
        scope: Scope,
        activation_token: Option<&str>,
    ) {
        let token = activation_token.filter(|token| !token.is_empty());
        let existing = PROPERTIES_WINDOWS.with(|windows| {
            let mut map = windows.borrow_mut();
            if let Some(handle) = map.get(&path)
                && handle.is_open()
            {
                return Some(handle.clone());
            }
            map.remove(&path);
            None
        });
        if let Some(handle) = existing {
            if let Some(token) = token {
                gds::ui::window::activate_with_token(&handle, token);
            }
            handle.focus();
            return;
        }

        let Some(windows) = try_use_windows() else {
            return;
        };

        let title = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| path.display().to_string());
        let window_path = path.clone();
        let mut options = workspace_options(title.clone(), None)
            .with_size(420.0, 640.0)
            .with_min_size(360.0, 420.0);
        if let Some(token) = token {
            options = options.with_activation_token(token.to_owned());
        }

        let handle = windows.open(options, move || {
            let _ = Gds::current().appearance().install_window_sheet();
            install_details_stylesheet();
            let props = Properties::new(window_path.clone(), scope.clone());
            let title_text = title.clone();
            let root_ref = NodeRef::new();
            let close_win = try_use_window();
            let ctrl_w = Shortcut::new(Key::character("w"), Modifiers::CONTROL);

            if let Some(clock) = zgui::view::Timers::current() {
                let focus_root = root_ref;
                drop(clock.request_frame(move |_| {
                    if focus_root.is_bound() {
                        focus_root.focus();
                    }
                }));
            }

            let props_body = props.view(false);
            view! {
                box(
                    node_ref = root_ref,
                    class = "gds-files-properties-root",
                    tabindex = Focus::Sequential,
                    on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                        if ev.repeat {
                            return;
                        }
                        let is_esc = ev.key == Key::Named(NamedKey::Escape) && ev.modifiers.is_empty();
                        let is_ctrl_w = ctrl_w.matches(&ev.key, ev.modifiers)
                            || ctrl_w.matches(&ev.key_without_modifiers, ev.modifiers);
                        if (is_esc || is_ctrl_w)
                            && let Some(ref win) = close_win
                        {
                            win.close();
                            ev.prevent_default();
                            ev.stop_propagation();
                        }
                    },
                ) {
                    Frame(without_sidebar = true) {
                        TitleBar(title = title_text) {}
                        {props_body}
                    }
                }
            }
        });

        PROPERTIES_WINDOWS.with(|windows| {
            windows.borrow_mut().insert(path, handle);
        });
    }

    fn reload(&self) {
        self.0.entry.set(EntryState::Loading);
        let weak = Rc::downgrade(&self.0);
        let path = self.0.path.clone();
        let task = self
            .0
            .scope
            .spawn_blocking_named("Inspect item properties", move || {
                Entry::inspect(&path).map_err(|err| err.to_string())
            });
        drop(spawn_local(async move {
            let result = task
                .await
                .map_err(|err| err.to_string())
                .and_then(std::convert::identity);
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let target = Self(inner);
            match result {
                Ok(entry) => {
                    let is_file = entry.kind == EntryKind::File;
                    let is_symlink = entry.kind == EntryKind::SymbolicLink;
                    target.0.entry.set(EntryState::Ready(entry));
                    if is_file {
                        target.load_origin_url();
                    }
                    if cfg!(target_os = "linux") && !is_symlink {
                        target.load_comment();
                    }
                    if target.0.tags_enabled {
                        target.load_tags();
                    }
                }
                Err(message) => {
                    target.0.entry.set(EntryState::Failed(message));
                }
            }
        }));
    }

    fn load_origin_url(&self) {
        let weak = Rc::downgrade(&self.0);
        let path = self.0.path.clone();
        let task = self
            .0
            .scope
            .spawn_blocking_named("Read file origin URL", move || {
                file_tags::read_origin_url(&path)
            });
        drop(spawn_local(async move {
            let Ok(origin) = task.await else {
                return;
            };
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.origin_url.set(origin);
        }));
    }

    fn load_comment(&self) {
        let weak = Rc::downgrade(&self.0);
        let path = self.0.path.clone();
        let task = self
            .0
            .scope
            .spawn_blocking_named("Read file comment", move || {
                file_tags::read_comment(&path).map_err(|err| err.to_string())
            });
        drop(spawn_local(async move {
            let result = task
                .await
                .map_err(|err| err.to_string())
                .and_then(std::convert::identity);
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.comment.set(Some(result));
        }));
    }

    fn load_tags(&self) {
        let weak = Rc::downgrade(&self.0);
        let path = self.0.path.clone();
        let task = self
            .0
            .scope
            .spawn_blocking_named("Read properties tags", move || {
                file_tags::read(&path).map_err(|err| err.to_string())
            });
        drop(spawn_local(async move {
            let result = task
                .await
                .map_err(|err| err.to_string())
                .and_then(std::convert::identity);
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.tags.set(Some(result));
        }));
    }

    fn load_description(&self, mime: Arc<str>) {
        let weak = Rc::downgrade(&self.0);
        let task = self
            .0
            .scope
            .spawn_blocking_named("Load MIME description", move || {
                directory::mime_descriptions::preload_mime(&mime)
            });
        drop(spawn_local(async move {
            let Ok(true) = task.await else {
                return;
            };
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner
                .description_version
                .update(|version| *version = version.wrapping_add(1));
        }));
    }

    fn format_label(entry: &Entry) -> Option<String> {
        let detailed = EntryPresentation::detailed_type(entry);
        (!detailed.is_empty() && detailed != UNKNOWN_VALUE).then_some(detailed)
    }

    fn base_rows(&self, entry: &Entry) -> Vec<AnyView> {
        let mut rows = Vec::new();
        rows.push(text_row("type", EntryPresentation::type_label(entry)));
        if let Some(format) = Self::format_label(entry) {
            rows.push(text_row("details-format", format));
        }
        if let Some(mime) = EntryPresentation::mime_type(entry) {
            rows.push(text_row("details-mime", mime.to_owned()));
        }
        rows.push(text_row("size", EntryPresentation::size_label(entry)));
        rows.push(metadata_row(
            "modified",
            AnyView::new(view! { {Timestamp::new(entry.modified)} }),
        ));
        if entry.details.accessed.is_some() {
            rows.push(metadata_row(
                "accessed",
                AnyView::new(view! { {Timestamp::new(entry.details.accessed)} }),
            ));
        }
        if entry.details.created.is_some() {
            rows.push(metadata_row(
                "details-created",
                AnyView::new(view! { {Timestamp::new(entry.details.created)} }),
            ));
        }
        let owner = EntryPresentation::owner(entry);
        rows.push(text_row("owner", owner.text));
        rows.push(text_row("group", EntryPresentation::group(entry)));
        if let Some(perms) = entry.details.permissions.as_ref() {
            rows.push(text_row("permissions", info::permissions(perms)));
        }
        if let Some(origin) = self.0.origin_url.get() {
            rows.push(text_row("details-origin", origin));
        }
        rows
    }

    fn fact_rows(metadata: &Metadata) -> Vec<AnyView> {
        let mut rows = Vec::new();
        match metadata {
            Metadata::File(FileFacts::Image(image)) => {
                if let Some(size) = image.size {
                    rows.push(text_row("details-dimensions", info::dimensions(size)));
                    if let Some(aspect) = info::aspect_ratio(size) {
                        rows.push(text_row("details-aspect", aspect));
                    }
                }
                if let Some(depth) = image.bit_depth {
                    rows.push(text_row("details-bit-depth", depth.to_string()));
                }
                if let Some(alpha) = image.alpha {
                    rows.push(text_row(
                        "details-alpha",
                        i18n::text(if alpha { "details-yes" } else { "details-no" }),
                    ));
                }
            }
            Metadata::File(FileFacts::Audio(audio)) => {
                if let Some(duration) = audio.duration {
                    rows.push(text_row("details-duration", info::duration(duration)));
                }
                if let Some(rate) = audio.sample_rate {
                    rows.push(text_row("details-sample-rate", info::sample_rate(rate)));
                }
                if let Some(channels) = audio.channels {
                    rows.push(text_row("details-channels", channels.to_string()));
                }
                if let Some(depth) = audio.bit_depth {
                    rows.push(text_row("details-bit-depth", depth.to_string()));
                }
                if let Some(bitrate) = audio.bitrate {
                    rows.push(text_row("details-bitrate", info::bitrate(bitrate)));
                }
                if let Some(title) = audio.tags.title.as_ref() {
                    rows.push(text_row("details-title", title.clone()));
                }
                if let Some(artist) = audio.tags.artist.as_ref() {
                    rows.push(text_row("details-artist", artist.clone()));
                }
                if let Some(album) = audio.tags.album.as_ref() {
                    rows.push(text_row("details-album", album.clone()));
                }
                if let Some(genre) = audio.tags.genre.as_ref() {
                    rows.push(text_row("details-genre", genre.clone()));
                }
                if let Some(year) = audio.tags.year {
                    rows.push(text_row("details-year", year.to_string()));
                }
                if let Some(track) = audio.tags.track {
                    rows.push(text_row("details-track", track.to_string()));
                }
            }
            Metadata::File(FileFacts::Video(video)) => {
                if let Some(duration) = video.duration {
                    rows.push(text_row("details-duration", info::duration(duration)));
                }
                if let Some(dims) = video.dimensions {
                    rows.push(text_row("details-dimensions", info::dimensions(dims)));
                    if let Some(aspect) = info::aspect_ratio(dims) {
                        rows.push(text_row("details-aspect", aspect));
                    }
                }
                if let Some(codec) = video.codec.as_ref() {
                    rows.push(text_row("details-codec", codec.clone()));
                }
            }
            Metadata::File(FileFacts::Text(text)) => {
                rows.push(text_row(
                    "details-encoding",
                    text.encoding.label().to_owned(),
                ));
                if let Some(endings) = text.line_endings {
                    let label = match info::line_endings(endings) {
                        info::LineEndingLabel::Literal(value) => value.to_owned(),
                        info::LineEndingLabel::Message(key) => i18n::text(key),
                    };
                    rows.push(text_row("details-line-endings", label));
                }
            }
            Metadata::File(FileFacts::Unknown) => {}
            Metadata::Directory(dir) => {
                rows.push(text_row(
                    "details-folders",
                    info::count(dir.folders, dir.capped),
                ));
                rows.push(text_row(
                    "details-files",
                    info::count(dir.files, dir.capped),
                ));
                rows.push(text_row(
                    "details-links",
                    info::count(dir.links, dir.capped),
                ));
            }
            Metadata::Symlink(link) => {
                rows.push(text_row(
                    "details-target",
                    link.target.display().to_string(),
                ));
            }
        }
        rows
    }

    fn comment_section(&self, entry_kind: EntryKind) -> Option<AnyView> {
        if !cfg!(target_os = "linux") || entry_kind == EntryKind::SymbolicLink {
            return None;
        }
        let comment_sig = self.0.comment;
        Some(AnyView::new(move || match comment_sig.get() {
            Some(Ok(Some(comment))) => AnyView::new(view! {
                column(class = "gds-files-details__tags-section") {
                    text(class = "gds-files-details__label") {{i18n::text("details-comment")}}
                    text(class = "gds-files-details__comment-text") {{comment}}
                }
            }),
            _ => AnyView::new(()),
        }))
    }

    fn tags_section(&self, entry_kind: EntryKind) -> Option<AnyView> {
        if !self.0.tags_enabled || entry_kind == EntryKind::SymbolicLink {
            return None;
        }
        let tags_sig = self.0.tags;
        Some(AnyView::new(move || match tags_sig.get() {
            Some(Ok(list)) if !list.is_empty() => {
                let chips: Vec<AnyView> = list
                    .into_iter()
                    .map(|tag| {
                        AnyView::new(view! {
                            text(class = "gds-files-details__tag-chip") {{tag}}
                        })
                    })
                    .collect();
                AnyView::new(view! {
                    column(class = "gds-files-details__tags-section") {
                        text(class = "gds-files-details__label") {{i18n::text("tags-title")}}
                        row(class = "gds-files-details__tags-list") {
                            {chips}
                        }
                    }
                })
            }
            _ => AnyView::new(()),
        }))
    }

    fn view(&self, panel_chrome: bool) -> AnyView {
        install_details_stylesheet();
        let entry_sig = self.0.entry;
        let desc_ver = self.0.description_version;
        let props = self.clone();
        let class_name = if panel_chrome {
            "gds-files-details"
        } else {
            "gds-files-properties-window"
        };
        let fallback_name = self
            .0
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.0.path.display().to_string());

        AnyView::new(view! {
            column(class = class_name) {
                {move || match entry_sig.get() {
                    EntryState::Loading => {
                        let hero = details_hero_view(
                            details_stage_icon_view(FileIcon::plain(IconName::Folder), true),
                            &fallback_name,
                            panel_chrome,
                        );
                        let body = AnyView::new(view! {
                            column(class = "gds-files-collection-state") {
                                Spinner(label = i18n::text("loading"))
                                text(class = "gds-files-details__empty-text") {{i18n::text("loading")}}
                            }
                        });
                        Either::Left(Either::Left(details_content_view(hero, body)))
                    }
                    EntryState::Failed(message) => {
                        let retry_props = props.clone();
                        let hero = details_hero_view(
                            details_stage_icon_view(FileIcon::plain(IconName::CircleAlert), false),
                            &fallback_name,
                            panel_chrome,
                        );
                        let body = AnyView::new(view! {
                            column(class = "gds-files-collection-state") {
                                text(class = "gds-files-details__empty-text") {{message}}
                                Button(
                                    variant = ButtonVariant::Outline,
                                    on:click = move |_| retry_props.reload(),
                                ) {{i18n::text("retry")}}
                            }
                        });
                        Either::Left(Either::Right(details_content_view(hero, body)))
                    }
                    EntryState::Ready(entry) => {
                        let _ = desc_ver.get();
                        let source = entry.inspection_source();
                        let snap = props.0.cache.request(&source, Request::Metadata);
                        let metadata_ref = match &snap.metadata {
                            InspectionState::Ready(meta) => Some(meta.as_ref()),
                            _ => None,
                        };
                        let icon = FileIcon::for_entry(&entry);
                        let is_dir = FileIcon::is_directory(&entry);
                        let stage = if let Some(thumb_src) = entry.thumbnail_source(&props.0.cache) {
                            let _ = props.0.cache.request(&thumb_src, Request::Thumbnail);
                            details_stage_media_view(props.0.cache.clone(), thumb_src, icon, is_dir)
                        } else {
                            details_stage_icon_view(icon, is_dir)
                        };
                        let hero = details_hero_view(stage, &entry.display_name, panel_chrome);

                        let mut all_rows = props.base_rows(&entry);
                        if let Some(meta) = metadata_ref {
                            all_rows.extend(Self::fact_rows(meta));
                        }
                        let comment_view = props.comment_section(entry.kind);
                        let tags_view = props.tags_section(entry.kind);
                        let body = AnyView::new(view! {
                            column(class = "gds-files-details__rows") {
                                {all_rows}
                            }
                            {comment_view}
                            {tags_view}
                        });

                        Either::Right(details_content_view(hero, body))
                    }
                }}
            }
        })
    }
}
