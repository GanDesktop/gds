//! Virtualized file list/table view, column headers, column resize drag, and shared row state.

use gds::app::Gds;
use gds::appearance::motion::{WORKSPACE_DURATION_MS, use_transition};
use gds::i18n;
use gds::ui::feedback::tooltip::{HintTooltipProps, wrap_tokens};
use gds::ui::list::ListSurfaceProps;
use gds::ui::page::{PageTableAreaProps, TABLE_INSET};
use gds::ui::time::TimestampViewProps;
use gds::ui::{IconName, IconProps};
use zgui::geom::{CssPx, Point};
use zgui::prelude::*;
use zgui::reactive::{
    RenderEffect, RwSignal, Signal, UnsyncCallback, enter_non_reactive_zone, on_cleanup_local,
};
use zgui::view::{CustomPropertyName, Scoped, current_host, events, install_stylesheet};
use zgui::vocab::{PointerButton, Role, SharedString};
use zgui::{css, view};
use zgui_ui::VirtualListProps;
use zgui_ui_primitives::Binding;

use super::super::{
    collections::{COLLECTION_MAX_COLUMNS, Collections, collection_column_class},
    keyboard::Keyboard,
};
use crate::{
    entities::file::{columns::Column, sorting::Direction, ui::EntryPresentation},
    pages::workspace::{
        navigation::EnrichmentRunner,
        operations::{Dispatch, LocalOps},
        panes::{DragHost, Inspection, Marquee, MenuHost, drag::FileDrag, marquee},
        projection::{Geometry, Rows},
        state::{
            context::{Context, local_memo},
            model::{Contents, Row, RowKey},
        },
    },
    shared::lib::clipboard::Clipboard,
};

const NAME_MIN_WIDTH: f32 = 180.0;
const NAME_DEFAULT_MAX_WIDTH: f32 = 500.0;
const NAME_WIDTH_FRACTION: f32 = 0.45;
const LIST_SHEET: &str = "gds-files-workspace-list";

const LIST_CSS: &str = css!(
    r#"
    .gds-files-list-surface {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        border-width: 0px;
    }

    .gds-files-table-area {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        border-width: 0px;
    }

    .gds-files-table-x[data-focus-visible="true"] {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-table-x {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow-x: hidden;
        overflow-y: hidden;
    }

    .gds-files-table-x[data-scroll-x="true"] {
        overflow-x: auto;
    }

    .gds-files-table-inner {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        width: 100%;
        min-height: 0px;
    }

    .gds-files-header-row {
        display: flex;
        flex: none;
        align-items: center;
        width: 100%;
        height: var(--gds-list-header-height, 30px);
        border-bottom: 1px solid var(--gds-border);
    }

    .gds-files-header-cell {
        position: relative;
        display: flex;
        flex: none;
        align-items: center;
        align-self: stretch;
        min-width: 0px;
        box-sizing: border-box;
    }

    .gds-files-header-button {
        display: flex;
        flex: 1 1 0px;
        align-items: center;
        align-self: stretch;
        justify-content: flex-start;
        gap: 4px;
        min-width: 0px;
        padding: 0px 12px;
        border-width: 0px;
        background-color: transparent;
        font-weight: 400;
        cursor: pointer;
    }

    .gds-files-header-button[data-name-col="true"] {
        padding-left: 40px;
    }

    .gds-files-header-button[data-align-end="true"] {
        justify-content: flex-end;
        text-align: right;
    }

    .gds-files-header-button[data-sortable="false"] {
        cursor: default;
    }

    .gds-files-header-button[data-sortable="true"]:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-header-label {
        flex: 0 1 auto;
        min-width: 0px;
        font-size: var(--gds-text-xs, 12px);
        font-weight: 400;
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-header-button:not([data-align-end="true"]) .gds-files-header-label {
        flex: 1 1 0px;
    }

    .gds-files-header-button[data-active="true"] .gds-files-header-label {
        color: var(--gds-foreground);
    }

    .gds-files-header-sort {
        display: inline-flex;
        flex-direction: column;
        flex: none;
        align-items: center;
        justify-content: center;
        width: 12px;
        height: 18px;
        color: var(--gds-muted-foreground);
        opacity: 0;
        transition: opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-header-button:hover .gds-files-header-sort,
    .gds-files-header-button:focus-visible .gds-files-header-sort {
        opacity: 1;
    }

    .gds-files-header-sort[data-active="true"] {
        opacity: 1;
        color: var(--gds-foreground);
    }

    .gds-files-header-resize {
        position: absolute;
        top: 0px;
        right: 0px;
        bottom: 0px;
        width: 5px;
        cursor: ew-resize;
        z-index: 2;
    }

    .gds-files-header-resize:hover {
        background-color: var(--gds-primary);
    }

    .gds-files-list-host {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-virtual-list {
        flex: 1 1 0px;
        min-height: 0px;
    }

    .gds-files-virtual-list .zui-virtual-list__row {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0px;
        box-sizing: border-box;
    }

    .gds-files-row-wrap:not([data-grid="true"]) {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        padding: var(--gds-list-selection-inset, 2px) 0px;
        box-sizing: border-box;
    }

    .gds-files-row-wrap:not([data-grid="true"]) > .gds-files-menu-wrap,
    .gds-files-row-wrap:not([data-grid="true"]) .gds-files-drag-row-host {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-row-content {
        display: flex;
        align-items: center;
        width: 100%;
        height: 100%;
        min-width: 0px;
        border-width: 0px;
        border-radius: 6px;
        user-select: none;
        cursor: pointer;
        transition:
            background-color var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            color var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-row-wrap:not([data-grid="true"]) .gds-files-row-content {
        flex: 1 1 0px;
        width: auto;
        height: auto;
        min-height: var(--gds-list-selection-height, 36px);
        align-items: center;
    }

    .gds-files-row-content > .zui-tooltip__trigger {
        display: flex;
        align-self: stretch;
        flex: none;
        min-width: 0px;
    }

    .gds-files-row-content:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-row-content[data-selected="true"],
    .gds-files-row-content[data-selected="true"]:hover {
        background-color: var(--gds-selection);
        color: var(--gds-selection-foreground);
    }

    .gds-files-row-content[data-cursor="true"]:not([data-selected="true"]) {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-row-content[data-hidden-entry="true"] .gds-files-name-cell,
    .gds-files-row-content[data-hidden-entry="true"] .gds-files-meta-cell,
    .gds-files-row-content[data-hidden-entry="true"] .gds-files-collection-cell {
        opacity: 0.65;
    }

    .gds-files-row-content[data-ghosted="true"] .gds-files-name-cell,
    .gds-files-row-content[data-ghosted="true"] .gds-files-meta-cell,
    .gds-files-row-content[data-ghosted="true"] .gds-files-collection-cell {
        opacity: 0.35;
    }

    .gds-files-name-cell {
        display: flex;
        flex: none;
        align-items: center;
        align-self: stretch;
        gap: 10px;
        padding: 0px 12px;
        min-width: 0px;
        box-sizing: border-box;
    }

    .gds-files-row-name-group {
        display: flex;
        flex: 1 1 0px;
        align-items: center;
        gap: 6px;
        min-width: 0px;
    }

    .gds-files-row-name-text {
        flex: 0 1 auto;
        min-width: 0px;
        font-size: var(--gds-text-sm, 14px);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-cut-icon {
        flex: none;
        color: var(--gds-muted-foreground);
    }

    .gds-files-row-content[data-selected="true"] .gds-files-cut-icon {
        color: var(--gds-selection-foreground);
    }

    .gds-files-meta-cell {
        display: flex;
        flex: none;
        align-items: center;
        align-self: stretch;
        justify-content: flex-start;
        min-width: 0px;
        padding: 0px 12px;
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        box-sizing: border-box;
    }

    .gds-files-meta-cell > *,
    .gds-files-meta-cell .zui-tooltip__trigger {
        display: flex;
        min-width: 0px;
        max-width: 100%;
        overflow: hidden;
    }

    .gds-files-meta-cell[data-align-end="true"] {
        justify-content: flex-end;
        text-align: right;
    }

    .gds-files-meta-cell[data-align-end="true"] .gds-files-meta-text {
        text-align: right;
    }

    .gds-files-row-content[data-selected="true"] .gds-files-meta-cell {
        color: var(--gds-selection-foreground);
    }

    .gds-files-meta-cell[data-current-owner="true"] {
        font-weight: 600;
    }

    .gds-files-meta-text {
        width: 100%;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-col-name.gds-files-col-name {
        flex: none;
        width: var(--gds-files-col-name);
        min-width: var(--gds-files-col-name);
        max-width: var(--gds-files-col-name);
        box-sizing: border-box;
    }

    .gds-files-col-modified.gds-files-col-modified {
        flex: none;
        width: var(--gds-files-col-modified);
        min-width: var(--gds-files-col-modified);
        max-width: var(--gds-files-col-modified);
        box-sizing: border-box;
    }

    .gds-files-col-accessed.gds-files-col-accessed {
        flex: none;
        width: var(--gds-files-col-accessed);
        min-width: var(--gds-files-col-accessed);
        max-width: var(--gds-files-col-accessed);
        box-sizing: border-box;
    }

    .gds-files-col-type.gds-files-col-type {
        flex: none;
        width: var(--gds-files-col-type);
        min-width: var(--gds-files-col-type);
        max-width: var(--gds-files-col-type);
        box-sizing: border-box;
    }

    .gds-files-col-size.gds-files-col-size {
        flex: none;
        width: var(--gds-files-col-size);
        min-width: var(--gds-files-col-size);
        max-width: var(--gds-files-col-size);
        box-sizing: border-box;
    }

    .gds-files-col-owner.gds-files-col-owner {
        flex: none;
        width: var(--gds-files-col-owner);
        min-width: var(--gds-files-col-owner);
        max-width: var(--gds-files-col-owner);
        box-sizing: border-box;
    }

    .gds-files-col-group.gds-files-col-group {
        flex: none;
        width: var(--gds-files-col-group);
        min-width: var(--gds-files-col-group);
        max-width: var(--gds-files-col-group);
        box-sizing: border-box;
    }

    .gds-files-col-detailed-type.gds-files-col-detailed-type {
        flex: none;
        width: var(--gds-files-col-detailed-type);
        min-width: var(--gds-files-col-detailed-type);
        max-width: var(--gds-files-col-detailed-type);
        box-sizing: border-box;
    }

    .gds-files-col-permissions.gds-files-col-permissions {
        flex: none;
        width: var(--gds-files-col-permissions);
        min-width: var(--gds-files-col-permissions);
        max-width: var(--gds-files-col-permissions);
        box-sizing: border-box;
    }

    .gds-files-directory-surface {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-drop-background {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-empty-overlay {
        position: absolute;
        inset: 0px;
        display: flex;
        align-items: center;
        justify-content: center;
        pointer-events: none;
        color: var(--gds-muted-foreground);
    }
    "#
);

pub(super) fn install_list_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(LIST_SHEET, LIST_CSS);
    }
}

#[derive(Clone, PartialEq)]
enum HeaderStructure {
    Collection {
        columns: Vec<&'static str>,
        sortable: Vec<Column>,
        sort: crate::entities::file::sorting::Sort,
        lang: Option<gds::i18n::Language>,
    },
    Directory {
        columns: crate::entities::file::columns::Layout,
        sortable: Vec<Column>,
        sort: crate::entities::file::sorting::Sort,
        lang: Option<gds::i18n::Language>,
    },
}

#[derive(Clone, Copy)]
struct ColumnWidthSpec {
    min: f32,
    default: f32,
}

const fn col_spec(min: f32, default: f32) -> ColumnWidthSpec {
    ColumnWidthSpec { min, default }
}

pub(super) struct PreparedRow {
    pub(super) row: Row,
    columns: crate::entities::file::columns::Layout,
    pub(super) hidden: bool,
    pub(super) key: RowKey,
    pub(super) selected_sig: Signal<bool, LocalStorage>,
    pub(super) cursor_sig: Signal<bool, LocalStorage>,
    pub(super) selection_progress: Signal<f32, LocalStorage>,
    pub(super) label: String,
    pub(super) desc_attrs: Attrs,
    pub(super) is_cut: Signal<bool, LocalStorage>,
    pub(super) ghosted: Signal<bool, LocalStorage>,
}

const fn column_width_class(column: Column) -> &'static str {
    match column {
        Column::Name => "gds-files-col-name",
        Column::Modified => "gds-files-col-modified",
        Column::Accessed => "gds-files-col-accessed",
        Column::Type => "gds-files-col-type",
        Column::Size => "gds-files-col-size",
        Column::Owner => "gds-files-col-owner",
        Column::Group => "gds-files-col-group",
        Column::DetailedType => "gds-files-col-detailed-type",
        Column::Permissions => "gds-files-col-permissions",
    }
}

fn table_column_attrs(ctx: &Context) -> Attrs {
    let mut attrs = Attrs::new();
    for column in Column::ALL {
        let ctx = ctx.clone();
        let width = local_memo(move || column_width(&ctx, column));
        attrs = attrs.custom_property(
            CustomPropertyName::new(column_width_class(column)),
            move || Some(format!("{:.2}px", width.get())),
        );
    }
    let collection_ctx = ctx.clone();
    let collection_columns = local_memo(move || Collections(&collection_ctx).collection_columns());
    for index in 0..COLLECTION_MAX_COLUMNS {
        let width = local_memo(move || {
            collection_columns.with(|cols| {
                cols.as_ref()
                    .and_then(|items| items.get(index).map(|(_, w)| *w))
            })
        });
        attrs = attrs.custom_property(
            CustomPropertyName::new(collection_column_class(index)),
            move || width.get().map(|w| format!("{w:.2}px")),
        );
    }
    attrs
}

const fn column_width_spec(column: Column) -> ColumnWidthSpec {
    match column {
        Column::Name => col_spec(NAME_MIN_WIDTH, NAME_DEFAULT_MAX_WIDTH),
        Column::Modified | Column::Accessed => col_spec(144.0, 160.0),
        Column::Type => col_spec(96.0, 110.0),
        Column::Size => col_spec(88.0, 100.0),
        Column::Owner | Column::Group | Column::DetailedType => col_spec(90.0, 130.0),
        Column::Permissions => col_spec(84.0, 96.0),
    }
}

const fn column_min_width(column: Column) -> f32 {
    column_width_spec(column).min
}

fn table_viewport_width_untracked(ctx: &Context) -> f32 {
    (ctx.0.content_width.get_untracked() - TABLE_INSET * 2.0)
        .floor()
        .max(1.0)
}

fn resolve_column_width(column: Column, explicit: Option<f32>, viewport_width: f32) -> f32 {
    let spec = column_width_spec(column);
    if let Some(width) = explicit {
        return width.max(spec.min);
    }
    let fallback = if column == Column::Name {
        (viewport_width * NAME_WIDTH_FRACTION).clamp(NAME_MIN_WIDTH, NAME_DEFAULT_MAX_WIDTH)
    } else {
        spec.default
    };
    fallback.max(spec.min)
}

pub(super) fn column_width(ctx: &Context, column: Column) -> f32 {
    let explicit = ctx
        .0
        .column_widths
        .with(|widths| widths.get(&column).copied());
    let viewport = if explicit.is_none() && column == Column::Name {
        Geometry(ctx).table_viewport_width()
    } else {
        0.0
    };
    resolve_column_width(column, explicit, viewport)
}

pub(super) fn column_width_untracked(ctx: &Context, column: Column) -> f32 {
    let explicit = ctx
        .0
        .column_widths
        .with_untracked(|widths| widths.get(&column).copied());
    let viewport = if explicit.is_none() && column == Column::Name {
        table_viewport_width_untracked(ctx)
    } else {
        0.0
    };
    resolve_column_width(column, explicit, viewport)
}

fn content_columns_width(ctx: &Context) -> f32 {
    if let Some(columns) = Collections(ctx).collection_columns() {
        return columns.into_iter().map(|(_, width)| width).sum::<f32>();
    }
    ctx.read(|state| {
        state
            .tab()
            .columns
            .columns()
            .iter()
            .map(|column| column_width(ctx, *column))
            .sum::<f32>()
    })
}

fn table_width(ctx: &Context) -> f32 {
    content_columns_width(ctx).max(Geometry(ctx).table_viewport_width())
}

fn column_header(ctx: &Context, column: Column) -> AnyView {
    let sort_ctx = ctx.clone();
    let resize_drag = ctx.clone();
    let resize_cancel = ctx.clone();
    let resize_end = ctx.clone();
    let sortable = Rows(ctx).sortable_columns_untracked().contains(&column);
    let sorting = ctx.read_untracked(|state| state.tab().sort);
    let active = sortable && sorting.key == column;
    let ascending = sorting.direction == Direction::Ascending;
    let align_end = column == Column::Size;

    let drag_state: RwSignal<Option<(f32, f32)>, LocalStorage> = RwSignal::new_local(None);
    let header_cell_ref = NodeRef::new();
    let resize_ref = NodeRef::new();
    #[cfg(test)]
    {
        super::tests::record_header_cell_node(column, header_cell_ref);
        super::tests::record_header_resize_node(column, resize_ref);
    }
    let window = try_use_window();

    let sort_indicator = sortable.then(|| {
        AnyView::new(view! {
            box(
                class = "gds-files-header-sort",
                attr:data-active = active.then(|| "true".to_owned()),
                a11y:hidden = true
            ) {
                {if active {
                    let icon = if ascending {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    };
                    AnyView::new(view! {
                        Icon(name = icon, size = 12.0)
                    })
                } else {
                    AnyView::new(view! {
                        column(style:align-items = "center", style:justify-content = "center") {
                            Icon(name = IconName::ChevronUp, size = 8.0) {}
                            Icon(name = IconName::ChevronDown, size = 8.0) {}
                        }
                    })
                }}
            }
        })
    });

    let label_text = i18n::text(column.label_id());
    let label_node = AnyView::new(view! {
        text(class = "gds-files-header-label") {{label_text.clone()}}
    });

    AnyView::new(view! {
        row(
            node_ref = header_cell_ref,
            class = format!("gds-files-header-cell {}", column_width_class(column))
        ) {
            control(
                class = "gds-files-header-button",
                attr:data-name-col = (column == Column::Name).then(|| "true".to_owned()),
                attr:data-align-end = align_end.then(|| "true".to_owned()),
                attr:data-sortable = if sortable { "true" } else { "false" },
                attr:data-active = active.then(|| "true".to_owned()),
                tabindex = if sortable { Focus::Sequential } else { Focus::Programmatic },
                a11y:role = Role::Button,
                a11y:label = SharedString::from(label_text),
                a11y:disabled = !sortable,
                on:click = move |_| {
                    if sortable {
                        Dispatch(&sort_ctx).column_sort(column);
                    }
                }
            ) {
                {if align_end {
                    AnyView::new(view! {
                        {sort_indicator}
                        {label_node}
                    })
                } else {
                    AnyView::new(view! {
                        {label_node}
                        {sort_indicator}
                    })
                }}
            }
            box(
                node_ref = resize_ref,
                class = "gds-files-header-resize",
                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                    if ev.button != Some(PointerButton::Primary) || !ev.primary {
                        return;
                    }
                    let w = column_width_untracked(&resize_drag, column);
                    drag_state.set(Some((ev.position.x.0, w)));
                    ev.capture_pointer();
                    ev.stop_propagation();
                },
                on:pointer_move = {
                    let c = ctx.clone();
                    move |ev: &mut EventCx<'_, events::PointerMove>| {
                        let Some((start_x, base_w)) = drag_state.get_untracked() else {
                            return;
                        };
                        let delta = ev.position.x.0 - start_x;
                        c.0.column_widths.update(|widths| {
                            widths.insert(
                                column,
                                (base_w + delta).clamp(column_min_width(column), 900.0),
                            );
                        });
                    }
                },
                on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                    if drag_state.get_untracked().is_some() {
                        drag_state.set(None);
                        ev.release_pointer();
                        let widths = resize_end
                            .0
                            .column_widths
                            .get_untracked()
                            .into_iter()
                            .map(|(col, width)| (col.id().to_owned(), width))
                            .collect();
                        resize_end.update(|state| {
                            state
                                .places
                                .set_widths(state.tab().navigation.location().clone(), widths);
                        });
                        resize_end.save_places();
                    }
                },
                on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                    if let Some((_, original)) = drag_state.get_untracked() {
                        drag_state.set(None);
                        ev.release_pointer();
                        resize_cancel.0.column_widths.update(|widths| {
                            widths.insert(column, original);
                        });
                    }
                },
                on:pointer_enter = {
                    let win = window.clone();
                    move |_| {
                        if let Some(ref w) = win {
                            w.set_cursor(CursorStyle::ResizeEastWest);
                        }
                    }
                },
                on:pointer_leave = {
                    let win = window;
                    move |_| {
                        if drag_state.get_untracked().is_none()
                            && let Some(ref w) = win
                        {
                            w.set_cursor(CursorStyle::Default);
                        }
                    }
                }
            )
        }
    })
}

pub(super) fn attach_list_scroll_observer(ctx: &Context) {
    let list_node = ctx.0.list;
    let scroll_obs = list_node.observe_scroll();
    let ctx = ctx.clone();
    let scroll_fx = RenderEffect::new(move |_| {
        let pos = scroll_obs.get();
        if pos.scrollport.width.0 > 0.0 || pos.scrollport.height.0 > 0.0 {
            let s = list_node.scale().max(1.0);
            EnrichmentRunner(&ctx).viewport_changed(Point::new(
                CssPx(pos.offset.x.0 / s),
                CssPx(pos.offset.y.0 / s),
            ));
        }
    });
    on_cleanup_local(move || drop(scroll_fx));
}

pub(super) fn clear_blank_selection(ctx: &Context) {
    ctx.reset_typeahead();
    ctx.update_selection(|state| {
        let view = state.tab_mut().navigation.view_mut();
        view.selection.clear();
        view.anchor = None;
    });
}

pub(super) fn list_view(ctx: &Context) -> AnyView {
    let ctx = ctx.clone();
    AnyView::new(Scoped::new(move || list_view_scoped(&ctx)))
}

fn list_view_scoped(ctx: &Context) -> AnyView {
    attach_list_scroll_observer(ctx);

    let list_node = ctx.0.list;
    let viewport = NodeRef::new();
    let count_ctx = ctx.clone();
    let rows_ctx = ctx.clone();
    let header_ctx = ctx.clone();
    let table_ctx = ctx.clone();
    let overflow_ctx = ctx.clone();

    let row_h_ctx = ctx.clone();
    let hdr_h_ctx = ctx.clone();
    let row_height_sig = Signal::derive_local(move || Geometry(&row_h_ctx).list_row_height());
    let row_count =
        Signal::derive_local(move || count_ctx.read_structure(|state| state.tab().rows.len()));
    let header_h_sig = Signal::derive_local(move || {
        if row_count.get() == 0 {
            0.0
        } else {
            Geometry(&hdr_h_ctx).list_header_height()
        }
    });
    let empty = local_memo({
        let ctx = ctx.clone();
        move || Collections(&ctx).collection_empty_label()
    });
    let header_structure = local_memo({
        let ctx = ctx.clone();
        move || {
            let lang = Gds::try_current().map(|gds| gds.i18n().language());
            let is_collection = ctx.read_structure(|state| {
                matches!(
                    state.tab().contents,
                    Contents::Places(_)
                        | Contents::Recent(_)
                        | Contents::Trash(_)
                        | Contents::Tags { .. }
                        | Contents::Remote { .. }
                        | Contents::Volumes
                )
            });
            let sortable = Rows(&ctx).sortable_columns_untracked();
            if is_collection && let Some(columns) = Collections(&ctx).collection_column_ids() {
                let sort = ctx.read_structure(|state| state.tab().sort);
                HeaderStructure::Collection {
                    columns,
                    sortable,
                    sort,
                    lang,
                }
            } else {
                let (columns, sort) =
                    ctx.read_structure(|state| (state.tab().columns.clone(), state.tab().sort));
                HeaderStructure::Directory {
                    columns,
                    sortable,
                    sort,
                    lang,
                }
            }
        }
    });

    let has_marquee = ctx.read_untracked(|state| state.picker.is_none() && state.allows_multiple());
    let blank_press = (!has_marquee).then(|| {
        let clear_ctx = ctx.clone();
        UnsyncCallback::new(move |()| clear_blank_selection(&clear_ctx))
    });

    let focus_visible = ctx.0.list_focus_visible;
    let table_col_attrs = table_column_attrs(ctx);
    let target = AnyView::new(view! {
        ListSurface(
            scroll_ref = list_node,
            rows = row_count,
            row_height = row_height_sig,
            header_height = header_h_sig,
            inset_x = TABLE_INSET,
            on_blank_press = blank_press,
            class = "gds-files-list-surface"
        ) {
            PageTableArea(class = "gds-files-table-area", {..table_col_attrs}) {
                scroll(
                    node_ref = viewport,
                    class = "gds-files-table-x",
                    attr:data-focus-visible = move || {
                        focus_visible.get().then(|| "true".to_owned())
                    },
                    attr:data-scroll-x = move || {
                        (content_columns_width(&overflow_ctx)
                            > Geometry(&overflow_ctx).table_viewport_width() + 0.5)
                            .then(|| "true".to_owned())
                    }
                ) {
                    column(
                        class = "gds-files-table-inner",
                        style:min-width = move || Some(format!("{:.2}px", table_width(&table_ctx)))
                    ) {
                        {{move || {
                            let structure = header_structure.get();
                            let _z = enter_non_reactive_zone();
                            match structure {
                                HeaderStructure::Collection { .. } => {
                                    Collections(&header_ctx)
                                        .collection_header()
                                        .unwrap_or_else(|| AnyView::new(view! { box() }))
                                }
                                HeaderStructure::Directory { columns, .. } => {
                                    let cells = columns
                                        .columns()
                                        .iter()
                                        .map(|column| column_header(&header_ctx, *column))
                                        .collect::<Vec<_>>();
                                    AnyView::new(view! {
                                        row(class = "gds-files-header-row") {{cells}}
                                    })
                                }
                            }
                        }}}
                        box(class = "gds-files-list-host") {
                            VirtualList(
                                node_ref = Some(list_node),
                                count = row_count,
                                row_size = row_height_sig,
                                label = i18n::text("app-title"),
                                class = "gds-files-virtual-list",
                                row = move |index: usize| visible_list_row(&rows_ctx, index)
                            ) {}
                            {{move || empty.get().map(|label| AnyView::new(view! {
                                box(class = "gds-files-empty-overlay") {
                                    text {{i18n::text(label)}}
                                }
                            }))}}
                        }
                    }
                }
            }
        }
    });

    let dropped = DragHost(ctx).background_drop_target(target);
    let surface = if has_marquee {
        Marquee(ctx).marquee_surface(dropped, marquee::Layout::List { viewport })
    } else {
        dropped
    };
    MenuHost(ctx).directory_menu(surface)
}

pub(super) fn visible_row_slot(
    ctx: &Context,
    index: usize,
    is_grid: bool,
    render: impl Fn(&Context, usize) -> AnyView + 'static,
) -> AnyView {
    let slot_ctx = ctx.clone();
    let row_key = local_memo(move || {
        let revision = slot_ctx.0.revision.get();
        let state = slot_ctx.0.state.borrow();
        let tab = state.tab();
        let entry = tab.rows.get(index).map(Row::key);
        (tab.id, entry, tab.columns.clone(), revision)
    });
    let render_ctx = ctx.clone();
    AnyView::new(view! {
        box(
            class = "gds-files-row-wrap",
            attr:data-grid = is_grid.then(|| "true".to_owned())
        ) {
            {{move || {
                row_key.with(|_| ());
                let _z = enter_non_reactive_zone();
                render(&render_ctx, index)
            }}}
        }
    })
}

fn visible_list_row(ctx: &Context, index: usize) -> AnyView {
    visible_row_slot(ctx, index, false, list_row_view)
}

pub(super) fn prepare_row(ctx: &Context, index: usize) -> Option<PreparedRow> {
    let (row, columns, cut) = ctx.read_structure(|state| {
        let tab = state.tab();
        let Some(row) = tab.rows.get(index).cloned() else {
            return (None, tab.columns.clone(), None);
        };
        let cut = row.path().map(|path| path.to_owned());
        (Some(row), tab.columns.clone(), cut)
    });
    let row = row?;
    let hidden = match &row {
        Row::File(entry) => entry.is_hidden,
        Row::Remote(entry) => entry.name.starts_with('.'),
        Row::Place(location) => Rows(ctx)
            .place_entry(location)
            .is_some_and(|entry| entry.is_hidden),
        _ => false,
    };

    let key = row.key();
    let selected_sig = Signal::derive_local({
        let ctx = ctx.clone();
        let key = key.clone();
        move || {
            let _ = ctx.0.selection.get();
            let _ = ctx.0.revision.get();
            ctx.read_untracked(|s| s.tab().navigation.view().selection.contains(&key))
        }
    });
    let cursor_sig = Signal::derive_local({
        let ctx = ctx.clone();
        let key = key.clone();
        move || {
            if !ctx.0.list_focus_visible.get() {
                return false;
            }
            let _ = ctx.0.selection.get();
            let _ = ctx.0.revision.get();
            ctx.read_untracked(|s| s.tab().navigation.view().cursor.as_ref() == Some(&key))
        }
    });
    let selection_goal =
        Signal::derive_local(move || if selected_sig.get() { 1.0_f32 } else { 0.0_f32 });
    let selection_progress = use_transition(selection_goal, WORKSPACE_DURATION_MS);

    let label = row.label();
    let a11y_path = row.path().map(|path| path.display().to_string());
    let desc_attrs = a11y_path.as_ref().map_or_else(Attrs::new, |path| {
        Attrs::new()
            .a11y_from(A11yBinding::unspecified().description(SharedString::from(path.clone())))
    });

    let cut_for_badge = cut.clone();
    let is_cut = Signal::derive_local(move || {
        let _ = Clipboard::current().snapshot();
        cut_for_badge
            .as_ref()
            .is_some_and(|path| Clipboard::current().is_cut(path))
    });
    let drag_ctx = ctx.clone();
    let ghosted = Signal::derive_local(move || {
        let _ = Clipboard::current().snapshot();
        cut.as_ref().is_some_and(|path| {
            Clipboard::current().is_cut(path)
                || drag_ctx.is_drag_source(path)
                || FileDrag::is_source(path)
        })
    });

    Some(PreparedRow {
        row,
        columns,
        hidden,
        key,
        selected_sig,
        cursor_sig,
        selection_progress,
        label,
        desc_attrs,
        is_cut,
        ghosted,
    })
}

fn list_row_view(ctx: &Context, index: usize) -> AnyView {
    let Some(PreparedRow {
        row,
        columns,
        hidden,
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

    // Table row (`ViewMode::List` and collection tables)
    let selected_binding = Binding::controlled(selected_sig, |_| {});
    let cells: Vec<AnyView> =
        if let Some(collection_cells) = Collections(ctx).collection_cells(&row, selected_binding) {
            collection_cells
        } else {
            let label_tooltip = wrap_tokens(&label);
            let a11y_value = SharedString::from(label.clone());
            let select_label = ctx.clone();
            let open_label = ctx.clone();
            let label_view = AnyView::new(view! {
                row(class = "gds-files-row-name-group") {
                    text(
                        class = "gds-files-row-name-text",
                        a11y:role = Role::Label,
                        a11y:value = a11y_value,
                        a11y:selected = move || selected_sig.get(),
                        {..desc_attrs},
                        on:click = move |ev: &mut EventCx<'_, events::Click>| {
                            ev.stop_propagation();
                            Keyboard(&select_label).select_row(index, ev.modifiers);
                            select_label.focus_content();
                        },
                        on:double_click = move |ev: &mut EventCx<'_, events::DoubleClick>| {
                            ev.stop_propagation();
                            Dispatch(&open_label).open_row(index, false);
                        }
                    ) {{label}}
                    Show(when = move || is_cut.get()) {
                        Icon(
                            name = IconName::Scissors,
                            size = 12.0,
                            class = "gds-files-cut-icon"
                        )
                    }
                }
            });

            let row_artwork =
                Inspection(ctx).row_artwork_with_selection(&row, 18.0, selection_progress);
            let name_cell_ref = NodeRef::new();
            #[cfg(test)]
            super::tests::record_row_cell_node(index, Column::Name, name_cell_ref);
            let mut name = Some(AnyView::new(view! {
                HintTooltip(
                    text = label_tooltip,
                    trigger_class = column_width_class(Column::Name),
                ) {
                    row(
                        node_ref = name_cell_ref,
                        class = "gds-files-name-cell gds-files-col-name"
                    ) {
                        {row_artwork}
                        {label_view}
                    }
                }
            }));
            columns
                .columns()
                .iter()
                .map(|column| {
                    if *column == Column::Name {
                        name.take().unwrap_or_else(|| AnyView::new(view! { box() }))
                    } else {
                        cell(index, &row, *column)
                    }
                })
                .collect()
        };

    let target = AnyView::new(view! {
        row(
            class = "gds-files-row-content",
            attr:data-selected = move || selected_sig.get().then(|| "true".to_owned()),
            attr:data-cursor = move || cursor_sig.get().then(|| "true".to_owned()),
            attr:data-ghosted = move || ghosted.get().then(|| "true".to_owned()),
            attr:data-hidden-entry = hidden.then(|| "true".to_owned()),
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
        ) {{cells}}
    });

    let dragged = DragHost(ctx).file_row_drag(target, &row, None);
    MenuHost(ctx).row_menu(dragged, index)
}

fn cell(index: usize, row: &Row, column: Column) -> AnyView {
    #[cfg(not(test))]
    let _ = index;
    let current_owner = matches!(
        (row, column),
        (Row::File(entry), Column::Owner) if EntryPresentation::owner(entry).current
    );
    let align_end = column == Column::Size;

    let text_cell = |value: String| {
        let hint = wrap_tokens(&value);
        AnyView::new(view! {
            HintTooltip(text = hint) {
                text(class = "gds-files-meta-text") {{value}}
            }
        })
    };

    let content = match (row, column) {
        (Row::File(entry), Column::Modified) => AnyView::new(view! {
            TimestampView(value = Signal::stored_local(entry.modified))
        }),
        (Row::File(entry), Column::Accessed) => AnyView::new(view! {
            TimestampView(value = Signal::stored_local(entry.details.accessed))
        }),
        (Row::Trash(entry), Column::Modified) => AnyView::new(view! {
            TimestampView(value = Signal::stored_local(entry.deleted_at))
        }),
        (Row::File(entry), Column::Name) => text_cell(entry.display_name.clone()),
        (Row::File(entry), Column::Type) => text_cell(EntryPresentation::type_label(entry)),
        (Row::File(entry), Column::DetailedType) => {
            text_cell(EntryPresentation::detailed_type(entry))
        }
        (Row::File(entry), Column::Size) => text_cell(EntryPresentation::size_label(entry)),
        (Row::File(entry), Column::Owner) => text_cell(EntryPresentation::owner(entry).text),
        (Row::File(entry), Column::Group) => text_cell(EntryPresentation::group(entry)),
        (Row::File(entry), Column::Permissions) => entry.details.permissions.as_ref().map_or_else(
            || text_cell("—".to_owned()),
            |permissions| {
                let hint = permissions.octal();
                let sym = permissions.symbols();
                AnyView::new(view! {
                    HintTooltip(text = hint) {
                        text(class = "gds-files-meta-text") {{sym}}
                    }
                })
            },
        ),
        _ => text_cell(String::new()),
    };

    let cell_ref = NodeRef::new();
    #[cfg(test)]
    super::tests::record_row_cell_node(index, column, cell_ref);

    AnyView::new(view! {
        box(
            node_ref = cell_ref,
            class = format!("gds-files-meta-cell {}", column_width_class(column)),
            attr:data-current-owner = current_owner.then(|| "true".to_owned()),
            attr:data-align-end = align_end.then(|| "true".to_owned())
        ) {{content}}
    })
}
