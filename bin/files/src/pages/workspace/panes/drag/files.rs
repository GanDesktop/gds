//! File drag preview cards, drop targets, row drag handlers, and mouse back/forward navigation.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use gds::appearance::tokens::ICON_SIZE;
use gds::ui::{
    IconName, IconProps,
    drag::{
        DragCardProps, DragCardSurfaceProps, ExternalFileDrop, FileDropOptions, FileDropRejection,
        FileDropTarget, file_drop_target_attrs, render_drag_card_icon,
    },
};
use zgui::prelude::*;
use zgui::reactive::{Signal, UnsyncCallback, on_cleanup_local};
use zgui::view::{EventCx, events};
use zgui::vocab::PointerButton;
use zgui::{css, view};

use super::coordinator::{
    DragCoordinator, DragItem, DragRejectReason, DragSnapshot, FileDrag, FileDropOutcome,
    PREVIEW_VISIBLE_ITEMS, TargetKind, clear_pending_drag_rejection, coordinator_for,
    record_pending_drag_rejection,
};
use crate::{
    entities::file::{
        directory::{Entry, EntryKind},
        ui::FileIcon,
    },
    features::file_operations::{paste::PasteJob, transfer},
    pages::workspace::{
        navigation::Navigator,
        operations::PasteOps,
        projection::geometry::Grid,
        state::{
            context::Context,
            model::{Contents, Row, RowKey},
        },
    },
    shared::{
        i18n,
        lib::{clipboard, selection::Selection},
    },
};

const DRAG_PREVIEW_SHEET: &str = "gds-files-drag-preview";
const DRAG_PREVIEW_CSS: &str = css!(
    r#"
    .gds-files-drag-item {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        min-width: 0px;
    }

    .gds-files-drag-item__icon {
        flex-shrink: 0;
        color: var(--gds-muted-foreground);
    }

    .gds-files-drag-header__icon {
        flex-shrink: 0;
        color: var(--gds-primary);
    }

    .gds-files-drag-item__name {
        display: block;
        flex: 1 1 0%;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-drag-row-host {
        display: flex;
        flex-direction: column;
        min-width: 0px;
    }

    .gds-files-drag-row-host.gds-drop-target.gds-drop-target[data-drop-active="true"] {
        background: transparent;
        outline: none;
    }

    .gds-files-drag-row-host[data-drop-target="true"] .gds-files-row-content,
    .gds-files-drag-row-host[data-drop-active="true"] .gds-files-row-content,
    .gds-files-drag-row-host[data-drop-target="true"] .gds-files-row-content.gds-files-row-content,
    .gds-files-drag-row-host[data-drop-active="true"] .gds-files-row-content.gds-files-row-content,
    .gds-files-drag-row-host[data-drop-target="true"] .gds-files-tile,
    .gds-files-drag-row-host[data-drop-active="true"] .gds-files-tile,
    .gds-files-drag-row-host[data-drop-target="true"] .gds-files-tile.gds-files-tile,
    .gds-files-drag-row-host[data-drop-active="true"] .gds-files-tile.gds-files-tile {
        background-color: var(--gds-drag-target);
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-drag-tab-ghost.gds-files-drag-tab-ghost {
        position: fixed;
        pointer-events: none;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        box-sizing: border-box;
        border: none;
        border-radius: var(--gds-radius-control, 6px);
        background: var(--gds-panel);
        color: var(--gds-sidebar-foreground);
        --gds-nav-label-ink: var(--gds-sidebar-foreground);
        outline: 1px solid var(--gds-border);
        outline-offset: -1px;
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.24);
        overflow: hidden;
        user-select: none;
    }

    .gds-files-drag-tab-ghost.gds-files-drag-tab-ghost[data-selected="true"] {
        background: var(--gds-selection);
        color: var(--gds-selection-foreground);
        --gds-nav-label-ink: var(--gds-selection-foreground);
        outline: none;
    }

    .gds-files-drag-tab-ghost .gds-nav-item,
    .gds-files-drag-tab-ghost .gds-files-tab-action,
    .gds-files-drag-tab-ghost .gds-corner-action,
    .gds-files-drag-tab-ghost .zui-button {
        pointer-events: none;
    }

    .gds-files-sidebar-row-wrap[data-dragging-tab="true"],
    .gds-files-sidebar-row-wrap[data-dragging-quick-access="true"] {
        opacity: 0.45;
    }

    .gds-files-quick-access-drop-zone {
        display: flex;
        flex-direction: column;
        flex: 0 0 auto;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        border-radius: var(--gds-radius-control, 6px);
        transition: background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-quick-access-drop-zone[data-quick-access-drop="true"] {
        background-color: color-mix(in srgb, var(--gds-primary) 10%, transparent);
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }
    "#
);

pub(super) fn install_drag_preview_stylesheet() {
    if zgui::view::current_host().is_some() {
        zgui::view::install_stylesheet(DRAG_PREVIEW_SHEET, DRAG_PREVIEW_CSS);
    }
}

pub(super) fn render_file_drag_card_body(
    count: usize,
    items: Vec<DragItem>,
    copy_hint: Option<String>,
) -> AnyView {
    install_drag_preview_stylesheet();
    if count <= 1 {
        let (icon, label) = items
            .into_iter()
            .next()
            .map(|item| (item.icon.base(), item.name))
            .unwrap_or_else(|| (IconName::File, gds::i18n::count("item-count", 1)));
        AnyView::new(view! {
            column(class = "gds-drag-card__content") {
                row(class = "gds-files-drag-item") {
                    Icon(name = icon, size = ICON_SIZE, class = "gds-files-drag-item__icon")
                    text(class = "gds-files-drag-item__name") {{label}}
                }
            }
            {copy_hint.map(|hint| view! {
                row(class = "gds-drag-card__footer") {{hint}}
            })}
        })
    } else {
        let footer_text = if count > PREVIEW_VISIBLE_ITEMS {
            Some("…".to_owned())
        } else {
            copy_hint
        };
        let rows = items
            .into_iter()
            .take(PREVIEW_VISIBLE_ITEMS)
            .map(|item| {
                view! {
                    row(class = "gds-files-drag-item") {
                        Icon(
                            name = item.icon.base(),
                            size = ICON_SIZE,
                            class = "gds-files-drag-item__icon",
                        )
                        text(class = "gds-files-drag-item__name") {{item.name}}
                    }
                }
                .into_view()
            })
            .collect::<Vec<_>>();
        AnyView::new(view! {
            row(class = "gds-drag-card__header") {
                Icon(
                    name = FileIcon::multiple().base(),
                    size = ICON_SIZE,
                    class = "gds-files-drag-header__icon",
                )
                text(class = "gds-files-drag-item__name") {
                    {gds::i18n::count("item-count", count)}
                }
            }
            column(class = "gds-drag-card__content") {{rows}}
            {footer_text.map(|more| view! {
                row(class = "gds-drag-card__footer") {{more}}
            })}
        })
    }
}

pub(super) fn render_file_drag_card(
    drag: FileDrag,
    position: Signal<(f32, f32), LocalStorage>,
    copy_mode: Signal<bool, LocalStorage>,
) -> AnyView {
    let count = drag.count();
    let items = drag.preview_items();
    let copy_hint = copy_mode.get().then(|| i18n::text("copy-files"));
    let body = render_file_drag_card_body(count, items, copy_hint);
    AnyView::new(view! {
        DragCard(position = position) {{body}}
    })
}

fn report_drop_rejection(ctx: &Context, reason: DragRejectReason, rejection: FileDropRejection) {
    match rejection {
        FileDropRejection::Disallowed => ctx.report_notice(reason.message()),
        FileDropRejection::EmptyPaths => {
            ctx.report_notice(i18n::text_untracked("drop-no-local-files"));
        }
    }
}

fn folder_drop_target_attrs(
    ctx: &Context,
    coord: &Rc<DragCoordinator>,
    target_key: String,
    destination: PathBuf,
    internal_active: Signal<bool, LocalStorage>,
) -> Attrs {
    let gate_ctx = ctx.clone();
    let gate_dest = destination.clone();
    let can_accept = Rc::new(move || {
        if FileDrag::is_source_or_descendant(&gate_dest) {
            record_pending_drag_rejection(&gate_ctx, DragRejectReason::IntoSelf);
            false
        } else {
            true
        }
    });
    let spring_ctx = ctx.clone();
    let spring_coord = Rc::clone(coord);
    let spring_dest = destination.clone();
    let on_hover_change = UnsyncCallback::new(move |hovering: bool| {
        if hovering {
            spring_coord.enter_external_spring_target(
                &spring_ctx,
                target_key.clone(),
                TargetKind::Folder(spring_dest.clone()),
            );
        } else {
            spring_coord.leave_external_spring_target(&target_key);
        }
    });
    let drop_ctx = ctx.clone();
    let drop_coord = Rc::clone(coord);
    let on_external_drop = UnsyncCallback::new(move |drop: ExternalFileDrop| {
        drop_coord.clear_external_spring();
        clear_pending_drag_rejection();
        PasteOps(&drop_ctx).handle_external_file_drop(destination.clone(), drop);
    });
    let reject_ctx = ctx.clone();
    let reject_coord = Rc::clone(coord);
    let on_reject = UnsyncCallback::new(move |rejection: FileDropRejection| {
        reject_coord.clear_external_spring();
        clear_pending_drag_rejection();
        report_drop_rejection(&reject_ctx, DragRejectReason::IntoSelf, rejection);
    });
    file_drop_target_attrs(FileDropTarget {
        active: internal_active,
        options: FileDropOptions::copy_or_move(DragAction::Move),
        can_accept: Some(can_accept),
        on_hover_change: Some(on_hover_change),
        on_drop: Some(on_external_drop),
        on_reject: Some(on_reject),
    })
}

pub(super) fn background_drop_target(ctx: &Context, view: AnyView) -> AnyView {
    install_drag_preview_stylesheet();
    let coord = coordinator_for(ctx);
    let can_accept_ctx = ctx.clone();
    let can_accept = Rc::new(move || match background_rejection(&can_accept_ctx) {
        None => true,
        Some(reason) => {
            record_pending_drag_rejection(&can_accept_ctx, reason);
            false
        }
    });
    let drop_ctx = ctx.clone();
    let drop_coord = Rc::clone(&coord);
    let on_external_drop = UnsyncCallback::new(move |drop: ExternalFileDrop| {
        drop_coord.clear_external_spring();
        clear_pending_drag_rejection();
        let destination = drop_ctx.read_untracked(|state| {
            let tab = state.tab();
            matches!(tab.contents, Contents::Directory)
                .then(|| tab.navigation.path().map(Path::to_path_buf))
                .flatten()
        });
        if let Some(destination) = destination {
            PasteOps(&drop_ctx).handle_external_file_drop(destination, drop);
        } else {
            if let Some(handle) = drop.handle {
                handle.reject();
            }
            drop_ctx.report_notice(DragRejectReason::UnsupportedDestination.message());
        }
    });
    let reject_ctx = ctx.clone();
    let reject_coord = Rc::clone(&coord);
    let on_reject = UnsyncCallback::new(move |rejection: FileDropRejection| {
        reject_coord.clear_external_spring();
        clear_pending_drag_rejection();
        let reason = if matches!(rejection, FileDropRejection::Disallowed) {
            background_rejection(&reject_ctx).unwrap_or(DragRejectReason::UnsupportedDestination)
        } else {
            DragRejectReason::UnsupportedDestination
        };
        report_drop_rejection(&reject_ctx, reason, rejection);
    });
    let drop_attrs = file_drop_target_attrs(FileDropTarget {
        active: Signal::stored_local(false),
        options: FileDropOptions::copy_or_move(DragAction::Move),
        can_accept: Some(can_accept),
        on_hover_change: None,
        on_drop: Some(on_external_drop),
        on_reject: Some(on_reject),
    });

    AnyView::new(view! {
        box(class = "gds-files-drop-background", {..drop_attrs}) {
            {view}
        }
    })
}

pub(super) fn file_drag_target(ctx: &Context, view: AnyView, destination: PathBuf) -> AnyView {
    let coord = coordinator_for(ctx);
    let node = NodeRef::new();
    let instance = coord.alloc_token();
    let target_key = format!("dir:{instance}:{}", destination.display());
    let target_token = coord.register_target(
        target_key.clone(),
        node,
        TargetKind::Folder(destination.clone()),
    );
    let cleanup_coord = Rc::clone(&coord);
    on_cleanup_local(move || {
        cleanup_coord.unregister_target(target_token);
    });

    let internal_active = coord.hovered_signal(target_key.clone());
    let attrs = folder_drop_target_attrs(ctx, &coord, target_key, destination, internal_active);

    AnyView::new(view! {
        box(class = "gds-files-sidebar-row-wrap", node_ref = node, {..attrs}) {
            {view}
        }
    })
}

pub(super) fn unsupported_file_drop_target(ctx: &Context, view: AnyView) -> AnyView {
    let coord = coordinator_for(ctx);
    let node = NodeRef::new();
    let target_key = format!("unsupported:{}", coord.alloc_token());
    let target_token = coord.register_target(target_key, node, TargetKind::Unsupported);
    let cleanup_coord = Rc::clone(&coord);
    on_cleanup_local(move || {
        cleanup_coord.unregister_target(target_token);
    });

    let gate_ctx = ctx.clone();
    let can_accept = Rc::new(move || {
        record_pending_drag_rejection(&gate_ctx, DragRejectReason::UnsupportedDestination);
        false
    });
    let reject_ctx = ctx.clone();
    let reject_coord = Rc::clone(&coord);
    let on_reject = UnsyncCallback::new(move |rejection: FileDropRejection| {
        reject_coord.clear_external_spring();
        clear_pending_drag_rejection();
        report_drop_rejection(
            &reject_ctx,
            DragRejectReason::UnsupportedDestination,
            rejection,
        );
    });
    let attrs = file_drop_target_attrs(FileDropTarget {
        active: Signal::stored_local(false),
        options: FileDropOptions::copy_or_move(DragAction::Move),
        can_accept: Some(can_accept),
        on_hover_change: None,
        on_drop: None,
        on_reject: Some(on_reject),
    });

    AnyView::new(view! {
        box(class = "gds-files-sidebar-row-wrap", node_ref = node, {..attrs}) {
            {view}
        }
    })
}

pub(super) fn file_row_drag(
    ctx: &Context,
    view: AnyView,
    row: &Row,
    _grid: Option<Grid>,
) -> AnyView {
    let Row::File(entry) = row else {
        return view;
    };
    install_drag_preview_stylesheet();
    let coord = coordinator_for(ctx);
    let node = NodeRef::new();

    let dir_attrs = if entry.kind == EntryKind::Directory {
        let dest = entry.path.clone();
        let target_key = format!("row-dir:{}", dest.display());
        let target_token =
            coord.register_target(target_key.clone(), node, TargetKind::Folder(dest.clone()));
        let cleanup_coord = Rc::clone(&coord);
        on_cleanup_local(move || {
            cleanup_coord.unregister_target(target_token);
        });
        let internal_active = coord.hovered_signal(target_key.clone());
        folder_drop_target_attrs(ctx, &coord, target_key, dest, internal_active)
            .attribute(zgui::view::AttrName::new("data-drop-target"), move || {
                internal_active.get().then(|| "true".to_owned())
            })
    } else {
        Attrs::new()
    };

    let row_entry = entry.clone();
    let press_ctx = ctx.clone();
    let press_coord = Rc::clone(&coord);
    let move_ctx = ctx.clone();
    let move_coord = Rc::clone(&coord);
    let up_ctx = ctx.clone();
    let up_coord = Rc::clone(&coord);
    let cancel_ctx = ctx.clone();
    let cancel_coord = Rc::clone(&coord);

    AnyView::new(view! {
        box(
            class = "gds-files-drag-row-host",
            node_ref = node,
            {..dir_attrs},
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                press_coord.arm_file_drag(&press_ctx, &row_entry, ev);
            },
            on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                move_coord.move_file_drag(
                    &move_ctx,
                    ev,
                    |tab_id, entry| materialize_file_drag(&move_ctx, tab_id, entry),
                    |drag, paths| try_start_native_file_drag(&move_ctx, &move_coord, drag, paths),
                );
            },
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                if let Some(outcome) = up_coord.finish_file_drag(&up_ctx, ev) {
                    deliver_internal_file_drop(&up_ctx, outcome);
                }
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                cancel_coord.cancel_file_drag(&cancel_ctx, ev);
            },
        ) {
            {view}
        }
    })
}

pub(super) fn mouse_navigation(ctx: &Context, event: &mut EventCx<'_, events::PointerDown>) {
    match event.button {
        Some(PointerButton::Back) => {
            Navigator(ctx).history(false);
            event.prevent_default();
            event.stop_propagation();
        }
        Some(PointerButton::Forward) => {
            Navigator(ctx).history(true);
            event.prevent_default();
            event.stop_propagation();
        }
        _ => {}
    }
}

fn background_rejection(ctx: &Context) -> Option<DragRejectReason> {
    ctx.read_untracked(|state| {
        let tab = state.tab();
        if !matches!(tab.contents, Contents::Directory) {
            return Some(DragRejectReason::UnsupportedDestination);
        }
        match tab.navigation.path() {
            Some(dest) if FileDrag::is_source_or_descendant(dest) => {
                Some(DragRejectReason::IntoSelf)
            }
            Some(_) => None,
            None => Some(DragRejectReason::UnsupportedDestination),
        }
    })
}

fn try_start_native_file_drag(
    ctx: &Context,
    coord: &DragCoordinator,
    drag: &FileDrag,
    paths: &[PathBuf],
) -> bool {
    let Some(window) = ctx
        .0
        .window
        .as_ref()
        .filter(|window| window.capabilities().drag_source)
    else {
        return false;
    };
    let Ok((uri_list, plain_text)) = clipboard::encode_drag_payloads(paths) else {
        return false;
    };
    let mut request = DragSourceRequest::new(
        vec![
            DragMime::new("text/uri-list", uri_list),
            DragMime::new("text/plain;charset=utf-8", plain_text.clone()),
            DragMime::new("text/plain", plain_text.clone()),
            DragMime::new("UTF8_STRING", plain_text),
        ],
        DragActions::COPY | DragActions::MOVE,
    );
    if let Some(runtime) = ctx.scope().runtime_handle().cloned() {
        let export_paths = paths.to_vec();
        request =
            request.with_lazy_mime(gds::service::desktop::file_transfer::MIME_TYPE, move |_| {
                gds::service::desktop::file_transfer::export_files_blocking(
                    &runtime,
                    &export_paths,
                    true,
                    gds::service::desktop::file_transfer::DEFAULT_EXPORT_TIMEOUT,
                )
            });
    }
    let count = drag.count();
    let preview_items = drag.preview_items();
    if let Some(icon) = render_drag_card_icon(window, move || {
        let body = render_file_drag_card_body(count, preview_items, None);
        view! {
            DragCardSurface {{body}}
        }
    }) {
        request = request.with_icon(icon);
    }
    match window.start_drag(request) {
        Ok(session) => {
            clear_pending_drag_rejection();
            let _ = ctx.0.terminals.cancel_drag_hover();
            coord.begin_native_drag(ctx, session, paths.to_vec());
            true
        }
        Err(error) => {
            tracing::debug!(%error, "native start_drag declined; falling back to window-local drag");
            false
        }
    }
}

fn deliver_internal_file_drop(ctx: &Context, outcome: FileDropOutcome) {
    let destination = match outcome.kind {
        TargetKind::Folder(path) => Some(path),
        TargetKind::Tab(tab_id) => ctx.read_untracked(|state| {
            state
                .tabs
                .iter()
                .find(|tab| tab.id == tab_id)
                .filter(|tab| matches!(tab.contents, Contents::Directory))
                .and_then(|tab| tab.navigation.path().map(Path::to_path_buf))
        }),
        TargetKind::Unsupported => None,
    };
    let Some(destination) = destination else {
        ctx.report_notice(DragRejectReason::UnsupportedDestination.message());
        return;
    };
    let mode = if outcome.copy {
        transfer::Mode::Copy
    } else {
        transfer::Mode::Move
    };
    let paths = outcome.drag.paths();
    let invalid_self_target = paths
        .iter()
        .any(|src| destination == *src || destination.starts_with(src));
    let same_dir = mode == transfer::Mode::Move
        && paths
            .iter()
            .all(|p| p.parent() == Some(destination.as_path()));
    if invalid_self_target {
        ctx.report_notice(DragRejectReason::IntoSelf.message());
    } else if !paths.is_empty() && !same_dir {
        let job = PasteJob::new(destination, mode, paths);
        PasteOps(ctx).inspect_paste(job, false, true, None);
    }
}

fn materialize_file_drag(ctx: &Context, tab_id: u64, entry: &Entry) -> Option<FileDrag> {
    ctx.read_untracked(|state| {
        if state.picker.is_some() {
            return None;
        }
        let tab = state.tabs.iter().find(|tab| tab.id == tab_id)?;
        if !matches!(
            tab.contents,
            Contents::Directory | Contents::Recent(_) | Contents::Tags { .. }
        ) {
            return None;
        }
        let entries: Arc<[Entry]> = tab
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::File(item) => Some(item.clone()),
                _ => None,
            })
            .collect();
        let selected_paths: Selection<PathBuf> = tab
            .navigation
            .view()
            .selection
            .iter()
            .filter_map(|key| match key {
                RowKey::Local(path) => Some(path.clone()),
                _ => None,
            })
            .collect();
        let snapshot = DragSnapshot::new(&entries, &selected_paths);
        Some(snapshot.drag_for_row(entry, selected_paths.contains(&entry.path)))
    })
}
