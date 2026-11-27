//! Sidebar tab and quick-access drag ghosts, drop zones, and reorder pointer handlers.

#[cfg(test)]
use std::cell::RefCell;
use std::{path::PathBuf, rc::Rc};

use gds::appearance::tokens::{
    COLLAPSED_SIDEBAR_WIDTH, ICON_BUTTON_SIZE, SIDEBAR_ICON_SIZE, SIDEBAR_ITEM_HEIGHT,
    SIDEBAR_PADDING,
};
use gds::ui::{
    IconButtonProps, IconName, IconProps,
    navigation::{
        CornerActionProps, CornerButtonProps, NavItemContentProps, NavItemDetailsProps,
        NavItemHeaderProps, NavItemLabelProps, NavItemProps,
    },
    sidebar::SidebarPresentation,
};
use zgui::prelude::*;
use zgui::reactive::{Signal, on_cleanup_local};
use zgui::view::{EventCx, OverlayLayer, Portal, events};
use zgui::{component, view};

use super::coordinator::{
    QUICK_ACCESS_ZONE_KEY, QuickAccessDragPreview, TabDragPreview, TabDropAction, TargetKind,
    coordinator_for,
};
use super::files::install_drag_preview_stylesheet;
use crate::{
    entities::location::Location,
    pages::workspace::{
        navigation::{Navigator, QuickAccess},
        projection::{
            Rows,
            geometry::{ITEM_GAP, TAB_HEIGHT},
        },
        state::context::Context,
    },
};

#[cfg(test)]
thread_local! {
    static LAST_TAB_GHOST_NODE: RefCell<Option<NodeRef>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn last_tab_ghost_node() -> Option<NodeRef> {
    LAST_TAB_GHOST_NODE.with(|slot| *slot.borrow())
}

#[component]
pub(super) fn DraggedTabGhost(
    preview: TabDragPreview,
    position: Signal<(f32, f32), LocalStorage>,
    presentation: Signal<SidebarPresentation, LocalStorage>,
) -> impl IntoView {
    gds::ui::drag::install_drag_stylesheet();
    install_drag_preview_stylesheet();
    let collapsed_width = (COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING).max(36.0);
    let item_width = Signal::derive_local(move || {
        (presentation.get().width() - 2.0 * SIDEBAR_PADDING).max(collapsed_width)
    });
    let actions_visible = Signal::derive_local(move || presentation.get().actions_visible());
    let collapsed_rail = Signal::derive_local(move || presentation.get().is_collapsed());
    let grab_x = preview.grab_offset.0;
    let grab_y = preview.grab_offset.1;
    let ghost_ref = NodeRef::new();
    #[cfg(test)]
    LAST_TAB_GHOST_NODE.with(|slot| *slot.borrow_mut() = Some(ghost_ref));

    Portal::new(move || {
        let icon = preview.icon;
        let title = preview.title.clone();
        let detail_path = preview.detail_path.clone();
        let pinned = preview.pinned;
        let selected = preview.selected;
        AnyView::new(view! {
            box(class = "gds-drag-portal-overlay", a11y:hidden = true) {
                box(
                    node_ref = ghost_ref,
                    class = "gds-files-drag-tab-ghost",
                    attr:data-selected = selected.then(|| "true".to_owned()),
                    style:left = move || {
                        let width = item_width.get();
                        Some(format!("{:.2}px", position.get().0 - grab_x.min(width)))
                    },
                    style:top = move || {
                        Some(format!("{:.2}px", position.get().1 - grab_y.min(TAB_HEIGHT)))
                    },
                    style:width = move || Some(format!("{:.2}px", item_width.get())),
                    style:height = format!("{TAB_HEIGHT}px"),
                ) {
                    NavItem(
                        selected = selected,
                        presentation = presentation,
                        class = "gds-files-tab-item",
                        style:height = format!("{TAB_HEIGHT}px"),
                        var:--gds-files-tab-reserve = move || {
                            Some(format!(
                                "{:.2}px",
                                presentation.get().interpolate(ICON_BUTTON_SIZE + ITEM_GAP, 0.0)
                            ))
                        },
                    ) {
                        Icon(
                            name = icon,
                            size = SIDEBAR_ICON_SIZE,
                            class = "gds-files-nav-icon",
                        )
                        NavItemContent {
                            NavItemHeader {
                                NavItemLabel {{title}}
                            }
                            NavItemDetails {
                                {detail_path}
                            }
                        }
                        {pinned.then(|| AnyView::new(view! {
                            box(
                                class = "gds-files-tab-action",
                                attr:data-revealed = move || {
                                    actions_visible.get().then(|| "true".to_owned())
                                },
                                a11y:hidden = true,
                            ) {
                                IconButton(
                                    icon = IconName::Pin,
                                    label = Signal::stored_local(String::new()),
                                )
                            }
                            CornerAction(
                                revealed = collapsed_rail,
                                button = AnyView::new(view! {
                                    CornerButton(
                                        icon = IconName::Pin,
                                        label = Signal::stored_local(String::new()),
                                    )
                                }),
                            ) {}
                        }))}
                    }
                }
            }
        })
    })
    .layer(OverlayLayer::Popover)
}

#[component]
fn DraggedQuickAccessGhost(
    preview: QuickAccessDragPreview,
    position: Signal<(f32, f32), LocalStorage>,
    presentation: Signal<SidebarPresentation, LocalStorage>,
) -> impl IntoView {
    gds::ui::drag::install_drag_stylesheet();
    install_drag_preview_stylesheet();
    let collapsed_width = (COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING).max(36.0);
    let item_width = Signal::derive_local(move || {
        (presentation.get().width() - 2.0 * SIDEBAR_PADDING).max(collapsed_width)
    });
    let grab_x = preview.grab_offset.0;
    let grab_y = preview.grab_offset.1;

    Portal::new(move || {
        let icon = preview.icon;
        let label = preview.label.clone();
        let selected = preview.selected;
        AnyView::new(view! {
            box(class = "gds-drag-portal-overlay", a11y:hidden = true) {
                box(
                    class = "gds-files-drag-tab-ghost",
                    attr:data-selected = selected.then(|| "true".to_owned()),
                    style:left = move || {
                        let width = item_width.get();
                        Some(format!("{:.2}px", position.get().0 - grab_x.min(width)))
                    },
                    style:top = move || {
                        Some(format!("{:.2}px", position.get().1 - grab_y.min(SIDEBAR_ITEM_HEIGHT)))
                    },
                    style:width = move || Some(format!("{:.2}px", item_width.get())),
                    style:height = format!("{SIDEBAR_ITEM_HEIGHT}px"),
                ) {
                    NavItem(
                        selected = selected,
                        presentation = presentation,
                        class = "gds-files-location-item",
                        style:height = format!("{SIDEBAR_ITEM_HEIGHT}px"),
                    ) {
                        Icon(
                            name = icon,
                            size = SIDEBAR_ICON_SIZE,
                            class = "gds-files-nav-icon",
                        )
                        NavItemLabel {{label}}
                    }
                }
            }
        })
    })
    .layer(OverlayLayer::Popover)
}

pub(super) fn render_tab_ghost(
    preview: TabDragPreview,
    position: Signal<(f32, f32), LocalStorage>,
    presentation: Signal<SidebarPresentation, LocalStorage>,
) -> AnyView {
    AnyView::new(view! {
        DraggedTabGhost(
            preview = preview,
            position = position,
            presentation = presentation,
        )
    })
}

pub(super) fn render_quick_access_ghost(
    preview: QuickAccessDragPreview,
    position: Signal<(f32, f32), LocalStorage>,
    presentation: Signal<SidebarPresentation, LocalStorage>,
) -> AnyView {
    AnyView::new(view! {
        DraggedQuickAccessGhost(
            preview = preview,
            position = position,
            presentation = presentation,
        )
    })
}

pub(super) fn draggable_tab(ctx: &Context, view: AnyView, id: u64, pinned: bool) -> AnyView {
    let coord = coordinator_for(ctx);
    let node = NodeRef::new();
    let target_key = format!("tab:{id}");
    let tab_token = coord.register_tab(id, pinned, node);
    let target_token = coord.register_target(target_key.clone(), node, TargetKind::Tab(id));
    let cleanup_coord = Rc::clone(&coord);
    on_cleanup_local(move || {
        cleanup_coord.unregister_tab(tab_token);
        cleanup_coord.unregister_target(target_token);
    });

    let is_hovered = coord.hovered_signal(target_key.clone());
    let is_dragging_self = coord.dragging_tab_signal(id);

    let down_coord = Rc::clone(&coord);
    let down_presentation = ctx.sidebar_presentation();
    let move_coord = Rc::clone(&coord);
    let move_ctx = ctx.clone();
    let up_coord = Rc::clone(&coord);
    let up_ctx = ctx.clone();
    let cancel_coord = Rc::clone(&coord);
    let enter_ctx = ctx.clone();
    let enter_coord = Rc::clone(&coord);
    let enter_key = target_key.clone();
    let leave_coord = Rc::clone(&coord);
    let leave_key = target_key;

    AnyView::new(view! {
        box(
            class = "gds-files-sidebar-row-wrap",
            node_ref = node,
            attr:data-drop-active = move || is_hovered.get().then(|| "true".to_owned()),
            attr:data-dragging-tab = move || is_dragging_self.get().then(|| "true".to_owned()),
            on:drag_enter = move |ev| {
                if ev.offers_files() {
                    enter_coord.enter_external_spring_target(
                        &enter_ctx,
                        enter_key.clone(),
                        TargetKind::Tab(id),
                    );
                }
            },
            on:drag_leave = move |_| {
                leave_coord.leave_external_spring_target(&leave_key);
            },
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                down_coord.arm_tab_drag(id, pinned, ev, |pos| {
                    tab_grab_offset(&node, pos, down_presentation.get_untracked())
                });
            },
            on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                move_coord.move_tab_drag(
                    id,
                    pinned,
                    |armed_pinned, grab_offset| {
                        let (icon, title, detail_path, selected) =
                            tab_drag_snapshot(&move_ctx, id).unwrap_or_else(|| {
                                (IconName::Folder, String::new(), String::new(), false)
                            });
                        TabDragPreview {
                            id,
                            pinned: armed_pinned,
                            selected,
                            icon,
                            title,
                            detail_path,
                            grab_offset,
                        }
                    },
                    || QuickAccess(&move_ctx).tab_local_path(id).is_some(),
                    ev,
                );
            },
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                let Some(action) = up_coord.finish_tab_drag(id, ev) else {
                    return;
                };
                match action {
                    TabDropAction::Reorder { from_id, to_id } => {
                        let nav = Navigator(&up_ctx);
                        if let (Some(from_idx), Some(to_idx)) =
                            (nav.tab_index(from_id), nav.tab_index(to_id))
                        {
                            nav.reorder_tab(from_idx, to_idx);
                            ev.stop_propagation();
                        }
                    }
                    TabDropAction::AddQuickAccess { tab_id } => {
                        QuickAccess(&up_ctx).add_quick_access_from_tab(tab_id);
                        ev.stop_propagation();
                    }
                }
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                ev.release_pointer();
                cancel_coord.cancel_all();
            },
        ) {
            {view}
        }
    })
}

pub(super) fn quick_access_drop_zone(ctx: &Context, view: AnyView) -> AnyView {
    install_drag_preview_stylesheet();
    let coord = coordinator_for(ctx);
    let node = NodeRef::new();
    let token = coord.register_quick_access_zone(node);
    let cleanup_coord = Rc::clone(&coord);
    on_cleanup_local(move || {
        cleanup_coord.unregister_quick_access_zone(token);
    });
    let active = coord.hovered_signal(QUICK_ACCESS_ZONE_KEY.to_owned());

    AnyView::new(view! {
        column(
            node_ref = node,
            class = "gds-files-quick-access-drop-zone",
            attr:data-quick-access-drop = move || active.get().then(|| "true".to_owned()),
        ) {
            {view}
        }
    })
}

pub(super) fn draggable_quick_access(
    ctx: &Context,
    view: AnyView,
    path: PathBuf,
    label: String,
    icon: IconName,
) -> AnyView {
    let coord = coordinator_for(ctx);
    let node = NodeRef::new();
    let row_key = format!("qa:{}", path.display());
    let row_token = coord.register_quick_access_row(path.clone(), node);
    let cleanup_coord = Rc::clone(&coord);
    on_cleanup_local(move || {
        cleanup_coord.unregister_quick_access_row(row_token);
    });

    let is_hovered = coord.hovered_signal(row_key);
    let is_dragging_self = coord.dragging_quick_access_signal(path.clone());

    let down_coord = Rc::clone(&coord);
    let down_path = path.clone();
    let down_label = label;
    let down_presentation = ctx.sidebar_presentation();
    let move_coord = Rc::clone(&coord);
    let move_path = path.clone();
    let move_ctx = ctx.clone();
    let up_coord = Rc::clone(&coord);
    let up_path = path;
    let up_ctx = ctx.clone();
    let cancel_coord = Rc::clone(&coord);

    AnyView::new(view! {
        box(
            class = "gds-files-sidebar-row-wrap",
            node_ref = node,
            attr:data-drop-active = move || is_hovered.get().then(|| "true".to_owned()),
            attr:data-dragging-quick-access = move || is_dragging_self.get().then(|| "true".to_owned()),
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                down_coord.arm_quick_access_drag(&down_path, icon, &down_label, ev, |pos| {
                    row_grab_offset(
                        &node,
                        pos,
                        down_presentation.get_untracked(),
                        SIDEBAR_ITEM_HEIGHT,
                    )
                });
            },
            on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                move_coord.move_quick_access_drag(
                    &move_path,
                    |armed_path| {
                        move_ctx.read_untracked(|state| {
                            state.tab().navigation.path() == Some(armed_path)
                        })
                    },
                    ev,
                );
            },
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                if let Some(reorder) = up_coord.finish_quick_access_drag(&up_path, ev) {
                    QuickAccess(&up_ctx).reorder_quick_access_item(&reorder.from, &reorder.to);
                }
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                ev.release_pointer();
                cancel_coord.cancel_all();
            },
        ) {
            {view}
        }
    })
}

fn tab_drag_snapshot(ctx: &Context, id: u64) -> Option<(IconName, String, String, bool)> {
    let rows = Rows(ctx);
    ctx.read_untracked(|state| {
        let active_id = state.tab().id;
        let tab = state.tabs.iter().find(|tab| tab.id == id)?;
        let location = tab.navigation.location();
        let icon = location.icon();
        let title = match location {
            Location::Volume { identity, relative } if relative.is_empty() => rows
                .volume_at_location(identity)
                .map_or_else(|| rows.location_title(location), |volume| volume.name),
            _ => rows.location_title(location),
        };
        let detail_path = tab
            .navigation
            .path()
            .map_or_else(|| location.label(), |path| path.display().to_string());
        Some((icon, title, detail_path, active_id == id))
    })
}

fn tab_grab_offset(
    node: &NodeRef,
    pointer: (f32, f32),
    presentation: SidebarPresentation,
) -> (f32, f32) {
    row_grab_offset(node, pointer, presentation, TAB_HEIGHT)
}

fn row_grab_offset(
    node: &NodeRef,
    pointer: (f32, f32),
    presentation: SidebarPresentation,
    default_height: f32,
) -> (f32, f32) {
    let fallback_w = (presentation.width() - 2.0 * SIDEBAR_PADDING)
        .max(COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING);
    let Some(bounds) = node.window_bounds() else {
        return (fallback_w * 0.5, default_height * 0.5);
    };
    let scale = node.scale().max(0.01);
    let left = bounds.origin.x.0 / scale;
    let top = bounds.origin.y.0 / scale;
    let width = (bounds.size.width.0 / scale).max(1.0);
    let height = (bounds.size.height.0 / scale).max(1.0);
    (
        (pointer.0 - left).clamp(0.0, width),
        (pointer.1 - top).clamp(0.0, height),
    )
}
