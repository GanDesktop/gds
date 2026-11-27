//! Virtualized grid view, tile rendering, and `GridMotion` animation integration.

use gds::ui::feedback::tooltip::{HintTooltipProps, wrap_tokens};
use gds::ui::list::ListSurfaceProps;
use gds::ui::page::PageTableAreaProps;
use gds::ui::{IconName, IconProps};
use zgui::prelude::*;
use zgui::reactive::Signal;
use zgui::view::{Scoped, current_host, events, install_stylesheet};
use zgui::vocab::{PointerButton, Role, SharedString};
use zgui::{css, view};
use zgui_ui::Virtualize;

use super::super::keyboard::Keyboard;
use super::list::{
    PreparedRow, attach_list_scroll_observer, clear_blank_selection, prepare_row, visible_row_slot,
};
use crate::pages::workspace::{
    operations::{Dispatch, LocalOps},
    panes::{DragHost, Inspection, Marquee, MenuHost, drag::take_row_pointer_down, marquee},
    projection::{
        Geometry,
        geometry::{GRID_GAP, GRID_PADDING, GridMotion},
    },
    state::context::Context,
};

const GRID_SHEET: &str = "gds-files-workspace-grid";

const GRID_CSS: &str = css!(
    r#"
    .gds-files-grid-scroll[data-focus-visible="true"] {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-row-wrap[data-grid="true"] {
        width: auto;
        height: auto;
        padding: 0px;
        flex: none;
    }

    .gds-files-row-wrap[data-grid="true"] > .gds-files-menu-wrap,
    .gds-files-row-wrap[data-grid="true"] .gds-files-drag-row-host {
        width: auto;
        height: auto;
        flex: none;
    }

    .gds-files-grid-scroll {
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
        overflow-y: auto;
        overflow-x: hidden;
    }

    .gds-files-grid-pane {
        display: flex;
        flex-direction: column;
    }

    .gds-files-grid-row {
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .gds-files-tile {
        position: relative;
        display: flex;
        flex-direction: column;
        align-items: center;
        padding: 8px;
        gap: 8px;
        border-radius: 8px;
        border: 1px solid transparent;
        box-sizing: border-box;
        user-select: none;
        cursor: pointer;
        transition:
            background-color var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            color var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            border-color var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-tile:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-tile[data-selected="true"],
    .gds-files-tile[data-selected="true"]:hover {
        background-color: var(--gds-selection);
        color: var(--gds-selection-foreground);
        border-color: var(--gds-primary);
    }

    .gds-files-tile[data-cursor="true"] {
        border-color: var(--gds-primary);
    }

    .gds-files-tile[data-hidden-entry="true"] .gds-files-tile-artwork,
    .gds-files-tile[data-hidden-entry="true"] .gds-files-tile-name-wrap {
        opacity: 0.65;
    }

    .gds-files-tile[data-ghosted="true"] .gds-files-tile-artwork,
    .gds-files-tile[data-ghosted="true"] .gds-files-tile-name-wrap {
        opacity: 0.35;
    }

    .gds-files-tile-cut {
        position: absolute;
        top: 4px;
        right: 4px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        pointer-events: none;
        color: var(--gds-muted-foreground);
    }

    .gds-files-tile[data-selected="true"] .gds-files-tile-cut {
        color: var(--gds-selection-foreground);
    }

    .gds-files-tile-artwork {
        display: flex;
        flex: none;
        align-items: center;
        justify-content: center;
        width: 100%;
    }

    .gds-files-tile-name-wrap {
        display: flex;
        align-items: flex-start;
        justify-content: center;
        width: 100%;
        min-width: 0px;
        overflow: hidden;
        text-align: center;
    }

    .gds-files-tile-name {
        display: block;
        width: 100%;
        min-width: 0px;
        font-size: var(--gds-text-sm, 14px);
        white-space: normal;
        overflow-wrap: anywhere;
        overflow: hidden;
        text-overflow: ellipsis;
        text-align: center;
    }
    "#
);

pub(super) fn install_grid_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(GRID_SHEET, GRID_CSS);
    }
}

pub(super) fn grid_view(ctx: &Context) -> AnyView {
    let ctx = ctx.clone();
    AnyView::new(Scoped::new(move || grid_view_scoped(&ctx)))
}

fn grid_view_scoped(ctx: &Context) -> AnyView {
    attach_list_scroll_observer(ctx);

    let list_node = ctx.0.list;
    let motion = GridMotion::new(ctx);
    let motion_rows = motion.clone();
    let count_ctx = ctx.clone();
    let rows_ctx = ctx.clone();

    let grid_row_count = Signal::derive_local(move || {
        let total = count_ctx.read_structure(|state| state.tab().rows.len());
        let columns = motion.target.get().columns.max(1);
        total.div_ceil(columns)
    });
    let row_height = Signal::derive_local(move || motion.target.get().row_height.max(1.0));
    let seen = Virtualize::new(list_node, grid_row_count, row_height, 4);
    let window = Signal::derive_local(move || seen.window());
    let has_marquee = ctx.read_untracked(|state| state.picker.is_none() && state.allows_multiple());
    let blank_ctx = ctx.clone();

    let focus_visible = ctx.0.list_focus_visible;
    let target = AnyView::new(view! {
        ListSurface(
            scroll_ref = list_node,
            class = "gds-files-list-surface"
        ) {
            PageTableArea(class = "gds-files-table-area") {
                scroll(
                    node_ref = list_node,
                    class = "gds-files-grid-scroll",
                    attr:data-focus-visible = move || {
                        focus_visible.get().then(|| "true".to_owned())
                    },
                    on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                        if !has_marquee
                            && ev.button == Some(PointerButton::Primary)
                            && !ev.modifiers.shift()
                            && !ev.modifiers.primary()
                            && !take_row_pointer_down(&blank_ctx)
                        {
                            clear_blank_selection(&blank_ctx);
                        }
                    }
                ) {
                    column(
                        class = "gds-files-grid-pane",
                        style:padding-left = format!("{GRID_PADDING}px"),
                        style:padding-right = format!("{GRID_PADDING}px"),
                        style:padding-top = move || {
                            Some(format!("{:.1}px", window.try_get().map_or(0.0, |w| w.lead)))
                        },
                        style:padding-bottom = move || {
                            Some(format!("{:.1}px", window.try_get().map_or(0.0, |w| w.trail)))
                        }
                    ) {
                        for grid_row in move || window.try_get().map(|w| w.indices()).unwrap_or_default(),
                            key = |grid_row: &usize| *grid_row
                        {
                            {{
                                let ctx = rows_ctx.clone();
                                let row_motion = motion_rows.clone();
                                move || {
                                    let total = ctx.read_structure(|state| state.tab().rows.len());
                                    let grid = row_motion.target.get();
                                    let columns = grid.columns.max(1);
                                    let start = grid_row * columns;
                                    let end = ((grid_row + 1) * columns).min(total);
                                    let tiles = (start..end)
                                        .map(|index| visible_grid_tile(&ctx, index, row_motion.clone()))
                                        .collect::<Vec<_>>();
                                    view! {
                                        row(
                                            class = "gds-files-grid-row",
                                            style:height = format!("{:.1}px", grid.row_height),
                                            style:gap = format!("{GRID_GAP}px")
                                        ) {{tiles}}
                                    }
                                }
                            }}
                        }
                    }
                }
            }
        }
    });

    let dropped = DragHost(ctx).background_drop_target(target);
    let surface = if has_marquee {
        Marquee(ctx).marquee_surface(dropped, marquee::Layout::Grid)
    } else {
        dropped
    };
    MenuHost(ctx).directory_menu(surface)
}

fn visible_grid_tile(ctx: &Context, index: usize, motion: GridMotion) -> AnyView {
    visible_row_slot(ctx, index, true, move |ctx, idx| {
        grid_tile_view(ctx, idx, &motion)
    })
}

fn grid_tile_view(ctx: &Context, index: usize, motion: &GridMotion) -> AnyView {
    let Some(PreparedRow {
        row,
        hidden,
        key,
        selected_sig,
        cursor_sig,
        selection_progress,
        label,
        desc_attrs,
        is_cut,
        ghosted,
        ..
    }) = prepare_row(ctx, index)
    else {
        return AnyView::new(view! { box() });
    };

    let select_ctx = ctx.clone();
    let open_ctx = ctx.clone();
    let context_ctx = ctx.clone();

    let geometry = Geometry(ctx).grid_geometry_untracked();
    let artwork = Inspection(ctx).tile_artwork(&row, geometry.icon_size, selection_progress);
    let a11y_label = SharedString::from(label.clone());
    let tile_tooltip = wrap_tokens(&label);
    let tile_pres = motion.present_tile(key, index);
    let tile_width = tile_pres.width;
    let tile_height = tile_pres.height;
    let tile_offset = tile_pres.offset;
    let line_height = geometry.line_height;
    let artwork_height = geometry.artwork_height;
    let name_height = geometry.name_height;

    let tile = AnyView::new(view! {
        column(
            class = "gds-files-tile",
            style:width = move || Some(format!("{:.1}px", tile_width.get())),
            style:height = move || Some(format!("{:.1}px", tile_height.get())),
            style:left = move || {
                let dx = tile_offset.get().0;
                (dx.abs() > 0.05).then(|| format!("{dx:.1}px"))
            },
            style:top = move || {
                let dy = tile_offset.get().1;
                (dy.abs() > 0.05).then(|| format!("{dy:.1}px"))
            },
            style:padding = format!("{GRID_PADDING}px"),
            style:gap = format!("{GRID_GAP}px"),
            attr:data-selected = move || selected_sig.get().then(|| "true".to_owned()),
            attr:data-cursor = move || cursor_sig.get().then(|| "true".to_owned()),
            attr:data-ghosted = move || ghosted.get().then(|| "true".to_owned()),
            attr:data-hidden-entry = hidden.then(|| "true".to_owned()),
            a11y:role = Role::ListItem,
            a11y:label = a11y_label,
            a11y:selected = move || selected_sig.get(),
            {..desc_attrs},
            on:click = move |ev: &mut EventCx<'_, events::Click>| {
                Keyboard(&select_ctx).select_row(index, ev.modifiers);
                select_ctx.focus_content();
            },
            on:double_click = move |_: &mut EventCx<'_, events::DoubleClick>| {
                Dispatch(&open_ctx).open_row(index, false);
            },
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                if ev.button == Some(PointerButton::Secondary) {
                    LocalOps(&context_ctx).select_context(index);
                }
            }
        ) {
            Show(when = move || is_cut.get()) {
                box(class = "gds-files-tile-cut", a11y:hidden = true) {
                    Icon(name = IconName::Scissors, size = 12.0)
                }
            }
            box(
                class = "gds-files-tile-artwork",
                style:height = format!("{artwork_height:.1}px")
            ) {
                {artwork}
            }
            box(
                class = "gds-files-tile-name-wrap",
                style:height = format!("{name_height:.1}px")
            ) {
                text(
                    class = "gds-files-tile-name",
                    style:line-height = format!("{line_height:.1}px"),
                    style:max-height = format!("{:.1}px", line_height * 2.0)
                ) {{label}}
            }
        }
    });

    let tipped = AnyView::new(view! {
        HintTooltip(text = tile_tooltip) { {tile} }
    });
    let dragged = DragHost(ctx).file_row_drag(tipped, &row, Some(geometry));
    MenuHost(ctx).row_menu(dragged, index)
}
