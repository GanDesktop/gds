//! Sort toolbar controls and persisted column-layout customization dialog.

use std::{cell::Cell, rc::Rc};

use gds::i18n;
use gds::ui::button::{
    ButtonProps as GdsButtonProps, ButtonSize, ButtonVariant as GdsButtonVariant, IconButtonProps,
};
use gds::ui::drag::DRAG_THRESHOLD;
use gds::ui::feedback::dialog::{
    DialogActionsProps, DialogDecision, DialogFooterProps, DialogHeaderProps, DialogTitleProps,
};
use gds::ui::feedback::tooltip::HintTooltipProps;
use gds::ui::menu::{AnchoredMenuProps, MenuEntry, render_entries};
use gds::ui::{IconName, IconProps};
use zgui::prelude::*;
use zgui::reactive::{RwSignal, UnsyncCallback};
use zgui::view::{EventCx, current_host, events, install_stylesheet};
use zgui::vocab::PointerButton;
use zgui::{css, view};
use zgui_ui::{
    CheckboxProps, Checked, SelectContentProps, SelectItemProps, SelectProps, SelectTriggerProps,
    SelectTriggerSize, SelectValueProps,
};
use zgui_ui_primitives::{Align, Binding, Placement, Side};

use crate::{
    entities::{
        file::{
            ViewMode,
            columns::{Column, Layout},
            sorting::{Direction, Sort},
        },
        location::Location,
    },
    pages::workspace::{
        navigation::EnrichmentRunner,
        projection::Rows,
        state::{
            context::Context,
            menus::{ColumnScope, Surface},
        },
    },
    shared::ui::actions::Command,
};

const MENUS_SHEET: &str = "gds-files-workspace-menus";

const MENUS_CSS: &str = css!(
    r#"
    .gds-files-columns-body {
        display: flex;
        flex-direction: column;
        gap: 10px;
    }

    .gds-files-columns-title {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
    }

    .gds-files-columns-target-bar {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        min-width: 0;
    }

    .gds-files-columns-folder-wrap {
        display: flex;
        min-width: 0;
        flex: 1 1 0%;
    }

    .gds-files-columns-folder-wrap .zui-tooltip__trigger {
        display: flex;
        min-width: 0;
        max-width: 100%;
    }

    .gds-files-columns-folder-badge {
        display: inline-flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
        height: 28px;
        padding: 0 8px;
        border-radius: var(--gds-radius-control, 6px);
        background: var(--gds-surface);
        border: 1px solid var(--gds-border);
        color: var(--gds-foreground);
        font-size: var(--gds-text-xs, 12px);
        min-width: 0;
        max-width: 100%;
        box-sizing: border-box;
    }

    .gds-files-columns-folder-label {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        min-width: 0;
    }

    .gds-files-columns-scope-group {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
        flex-shrink: 0;
    }

    .gds-files-columns-scope-label {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        white-space: nowrap;
    }

    .gds-files-columns-list {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-height: 360px;
        overflow-y: auto;
        padding: 2px 0;
    }

    .gds-files-columns-group-heading {
        font-size: var(--gds-text-xs, 12px);
        font-weight: 600;
        color: var(--gds-muted-foreground);
        padding: 4px 6px 2px 6px;
    }

    .gds-files-column-option {
        position: relative;
        display: flex;
        flex-direction: row;
        align-items: center;
        height: 30px;
        padding: 0 6px;
        gap: 8px;
        border-radius: 6px;
        box-sizing: border-box;
        user-select: none;
    }

    .gds-files-column-option:hover {
        background: var(--gds-hover);
    }

    .gds-files-column-option[data-drop-before="true"]::before {
        content: "";
        position: absolute;
        left: 6px;
        right: 6px;
        top: -1px;
        height: 2px;
        border-radius: 1px;
        background: var(--gds-primary);
        pointer-events: none;
    }

    .gds-files-column-handle {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 18px;
        height: 18px;
        flex-shrink: 0;
        color: var(--gds-muted-foreground);
        cursor: grab;
    }

    .gds-files-column-handle[data-pinned="true"] {
        opacity: 0.35;
        cursor: default;
    }

    .gds-files-column-spacer {
        width: 18px;
        height: 18px;
        flex-shrink: 0;
    }

    .gds-files-column-name {
        flex: 1 1 0%;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-foreground);
    }

    .gds-files-column-name--hidden {
        color: var(--gds-muted-foreground);
    }

    .gds-files-columns-drop-end {
        position: relative;
        height: 10px;
        width: 100%;
    }

    .gds-files-columns-drop-end[data-drop-end="true"]::before {
        content: "";
        position: absolute;
        left: 6px;
        right: 6px;
        top: 4px;
        height: 2px;
        border-radius: 1px;
        background: var(--gds-primary);
        pointer-events: none;
    }

    .gds-files-columns-footer {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        width: 100%;
        gap: 8px;
    }

    .gds-files-columns-footer__right {
        margin-left: auto;
    }
    "#
);

fn install_menus_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(MENUS_SHEET, MENUS_CSS);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColumnDropSlot {
    Before(Column),
    End,
}

struct ColumnDragState {
    layout: RwSignal<Layout, LocalStorage>,
    dragging_col: RwSignal<Option<Column>, LocalStorage>,
    drop_slot: RwSignal<Option<ColumnDropSlot>, LocalStorage>,
    row_refs: Rc<Vec<(Column, NodeRef)>>,
}

fn move_column_pinned(layout: &mut Layout, column: Column, target_index: usize) {
    if column.is_required() {
        let _ = layout.move_to(column, 0);
        return;
    }
    let min_index = usize::from(layout.columns().first().is_some_and(|c| c.is_required()));
    let _ = layout.move_to(column, target_index.max(min_index));
}

fn resolve_drop_slot(
    column: Column,
    pointer_y: f32,
    row_refs: &[(Column, NodeRef)],
) -> Option<ColumnDropSlot> {
    let mut chosen = ColumnDropSlot::End;
    for (candidate, node) in row_refs {
        if let Some(bounds) = node.window_bounds() {
            let scale = node.scale().max(0.01);
            let mid_y = (bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale;
            if pointer_y < mid_y {
                chosen = ColumnDropSlot::Before(*candidate);
                break;
            }
        }
    }
    match chosen {
        ColumnDropSlot::Before(target) if target == column => None,
        other => Some(other),
    }
}

fn apply_column_reorder(layout: &mut Layout, column: Column, slot: ColumnDropSlot) {
    let cols = layout.columns();
    let Some(src) = cols.iter().position(|c| *c == column) else {
        return;
    };
    let target_idx = match slot {
        ColumnDropSlot::Before(target) => {
            let Some(dst) = cols.iter().position(|c| *c == target) else {
                return;
            };
            if src < dst {
                dst.saturating_sub(1)
            } else {
                dst
            }
        }
        ColumnDropSlot::End => cols.len().saturating_sub(1),
    };
    move_column_pinned(layout, column, target_idx);
}

fn configurable_columns(ctx: &Context) -> bool {
    Rows(ctx).configurable_columns()
}

fn apply_tab_sort(ctx: &Context, update_sort: impl FnOnce(Sort) -> Sort) {
    let id = Rows(ctx).apply_tab_sort(update_sort);
    EnrichmentRunner(ctx).enrich_directory(id);
}

fn set_sort_key(ctx: &Context, column: Column) {
    apply_tab_sort(ctx, |current| Sort {
        key: column,
        direction: current.direction,
    });
}

fn toggle_sort_direction(ctx: &Context) {
    apply_tab_sort(ctx, |current| Sort {
        key: current.key,
        direction: match current.direction {
            Direction::Ascending => Direction::Descending,
            Direction::Descending => Direction::Ascending,
        },
    });
}

pub(super) fn column_menu(ctx: &Context) -> AnyView {
    let ctx = ctx.clone();
    AnyView::new(view! {
        row(class = "gds-files-column-controls") {
            {move || column_controls(&ctx)}
        }
    })
}

fn column_controls(ctx: &Context) -> AnyView {
    let anchor = NodeRef::new();
    let columns_ctx = ctx.clone();
    let sort_click = ctx.clone();
    let entries_ctx = ctx.clone();
    let close_ctx = ctx.clone();
    let configurable = configurable_columns(ctx);
    let grid = ctx.read(|state| state.tab().mode == ViewMode::Grid);
    let sortable = !Rows(ctx).sortable_columns().is_empty();
    let open = Signal::derive_local({
        let ctx = ctx.clone();
        move || matches!(ctx.0.menus.surface.get(), Surface::Sort)
    });
    let sort_menu = AnyView::new(view! {
        AnchoredMenu(
            anchor = anchor,
            placement = Placement::new(Side::Bottom, Align::End),
            class = "gds-files-menu-content",
            open = Binding::controlled(open, move |next: bool| {
                if !next {
                    close_ctx.0.menus.close();
                }
            }),
        ) {
            {render_entries(sort_entries(&entries_ctx))}
        }
    });

    AnyView::new(view! {
        row(class = "gds-files-column-controls") {
            {{sortable.then(|| AnyView::new(view! {
                box(class = "gds-files-menu-anchor") {
                    IconButton(
                        node_ref = anchor,
                        icon = IconName::ArrowDownWideNarrow,
                        label = Signal::derive_local(|| i18n::text("sort")),
                        shortcut = Some(Command::Sort.shortcut()),
                        active = open,
                        on:click = move |_| show_sort_menu(&sort_click)
                    ) {}
                    {sort_menu}
                }
            }))}}
            {{configurable.then(|| AnyView::new(view! {
                IconButton(
                    icon = IconName::Columns3Cog,
                    label = Signal::derive_local(|| i18n::text("columns-configure")),
                    disabled = Signal::stored_local(grid),
                    shortcut = Some(Command::Columns.shortcut()),
                    on:click = move |_| Rows(&columns_ctx).configure_columns()
                )
            }))}}
        }
    })
}

pub(super) fn sort_entries(ctx: &Context) -> Vec<MenuEntry> {
    let sortable = Rows(ctx).sortable_columns_untracked();
    if sortable.is_empty() {
        return Vec::new();
    }
    let current_sort = ctx.read_untracked(|state| state.tab().sort);
    let mut entries: Vec<MenuEntry> = sortable
        .into_iter()
        .map(|column| {
            let weak = ctx.downgrade();
            MenuEntry::Checkbox {
                label: i18n::text_untracked(column.label_id()),
                shortcut: None,
                checked: current_sort.key == column,
                disabled: false,
                on_toggle: UnsyncCallback::new(move |_| {
                    if let Some(ctx) = weak.upgrade() {
                        ctx.0.owner.with(|| set_sort_key(&ctx, column));
                    }
                }),
            }
        })
        .collect();

    entries.push(MenuEntry::Separator);
    let weak = ctx.downgrade();
    entries.push(MenuEntry::Checkbox {
        label: i18n::text_untracked("sort-descending"),
        shortcut: None,
        checked: current_sort.direction == Direction::Descending,
        disabled: false,
        on_toggle: UnsyncCallback::new(move |_| {
            if let Some(ctx) = weak.upgrade() {
                ctx.0.owner.with(|| toggle_sort_direction(&ctx));
            }
        }),
    });
    entries
}

pub(super) fn show_sort_menu(ctx: &Context) {
    if Rows(ctx).sortable_columns_untracked().is_empty() {
        return;
    }
    ctx.0.menus.surface.update(|surface| {
        *surface = if matches!(surface, Surface::Sort) {
            Surface::Closed
        } else {
            Surface::Sort
        };
    });
}

fn visible_column_row(
    column: Column,
    index: usize,
    len: usize,
    min_movable: usize,
    drag: &ColumnDragState,
) -> AnyView {
    let required = column.is_required();
    let row_ref = drag
        .row_refs
        .iter()
        .find(|(c, _)| *c == column)
        .map(|(_, r)| *r)
        .unwrap_or_else(NodeRef::new);
    let refs_for_move = Rc::clone(&drag.row_refs);
    let armed_start: Rc<Cell<Option<f32>>> = Rc::new(Cell::new(None));
    let a_down = Rc::clone(&armed_start);
    let a_move = Rc::clone(&armed_start);
    let a_up = Rc::clone(&armed_start);
    let a_cancel = Rc::clone(&armed_start);

    let layout = drag.layout;
    let dragging_col = drag.dragging_col;
    let drop_slot = drag.drop_slot;

    let is_checked = Signal::derive_local(move || {
        if layout.with(|l| l.contains(column)) {
            Checked::Yes
        } else {
            Checked::No
        }
    });
    let up_disabled = required || index <= min_movable;
    let down_disabled = required || index + 1 >= len;

    AnyView::new(view! {
        row(
            node_ref = row_ref,
            class = "gds-files-column-option",
            attr:data-drop-before = move || {
                (!required && drop_slot.get() == Some(ColumnDropSlot::Before(column)))
                    .then(|| "true".to_owned())
            },
        ) {
            box(
                class = "gds-files-column-handle",
                attr:data-pinned = required.then(|| "true".to_owned()),
                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                    if required || ev.button != Some(PointerButton::Primary) {
                        return;
                    }
                    a_down.set(Some(ev.position.y.0));
                    ev.capture_pointer();
                    ev.stop_propagation();
                    ev.prevent_default();
                },
                on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                    let y = ev.position.y.0;
                    if dragging_col.get_untracked().is_none() {
                        if let Some(start_y) = a_move.get()
                            && (y - start_y).abs() >= DRAG_THRESHOLD
                        {
                            dragging_col.set(Some(column));
                        } else {
                            return;
                        }
                    }
                    if dragging_col.get_untracked() != Some(column) {
                        return;
                    }
                    drop_slot.set(resolve_drop_slot(column, y, &refs_for_move));
                },
                on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                    a_up.set(None);
                    ev.release_pointer();
                    let dragged = dragging_col.get_untracked();
                    let slot = drop_slot.get_untracked();
                    dragging_col.set(None);
                    drop_slot.set(None);
                    if dragged == Some(column)
                        && let Some(slot) = slot
                    {
                        layout.update(|l| apply_column_reorder(l, column, slot));
                    }
                },
                on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                    a_cancel.set(None);
                    ev.release_pointer();
                    dragging_col.set(None);
                    drop_slot.set(None);
                },
            ) {
                Icon(name = IconName::GripVertical, size = 14.0) {}
            }
            Checkbox(
                checked = Binding::controlled(
                    is_checked,
                    move |next: Checked| {
                        layout.update(|l| {
                            l.set_visible(column, next == Checked::Yes);
                        });
                    }
                ),
                disabled = Signal::stored_local(required),
                class = "gds-files-column-check"
            )
            text(class = "gds-files-column-name") {{i18n::text(column.label_id())}}
            IconButton(
                icon = IconName::ArrowUp,
                label = i18n::text("columns-move-up"),
                size = ButtonSize::IconXs,
                disabled = Signal::stored_local(up_disabled),
                on:click = move |_| {
                    layout.update(|l| {
                        move_column_pinned(l, column, index.saturating_sub(1));
                    });
                }
            )
            IconButton(
                icon = IconName::ArrowDown,
                label = i18n::text("columns-move-down"),
                size = ButtonSize::IconXs,
                disabled = Signal::stored_local(down_disabled),
                on:click = move |_| {
                    layout.update(|l| {
                        move_column_pinned(l, column, index + 1);
                    });
                }
            )
        }
    })
}

fn hidden_column_row(column: Column, layout: RwSignal<Layout, LocalStorage>) -> AnyView {
    let is_checked = Signal::derive_local(move || {
        if layout.with(|l| l.contains(column)) {
            Checked::Yes
        } else {
            Checked::No
        }
    });
    AnyView::new(view! {
        row(class = "gds-files-column-option") {
            box(class = "gds-files-column-spacer") {}
            Checkbox(
                checked = Binding::controlled(
                    is_checked,
                    move |next: Checked| {
                        layout.update(|l| {
                            l.set_visible(column, next == Checked::Yes);
                        });
                    }
                ),
                class = "gds-files-column-check"
            )
            text(class = "gds-files-column-name gds-files-column-name--hidden") {
                {i18n::text(column.label_id())}
            }
        }
    })
}

pub(super) fn columns_dialog(ctx: &Context) -> AnyView {
    install_menus_stylesheet();
    let Surface::Columns {
        location,
        layout,
        scope,
    } = ctx.0.menus.surface.get()
    else {
        return AnyView::new(view! { box() });
    };

    let apply_ctx = ctx.clone();
    let defaults = ctx.clone();
    let has_override_ctx = ctx.clone();
    let restore_loc = location.clone();
    let override_loc = location.clone();
    let apply = location.clone();
    let confirm = ctx.clone();

    let scope_value = Signal::derive_local(move || scope.get().as_value().to_owned());
    let folder_name = Rows(ctx).location_title(&location);
    let full_address = location.display_address();
    let has_override = Signal::derive_local(move || {
        has_override_ctx.read(|state| state.places.columns_for(&override_loc).is_some())
    });

    let dragging_col: RwSignal<Option<Column>, LocalStorage> = RwSignal::new_local(None);
    let drop_slot: RwSignal<Option<ColumnDropSlot>, LocalStorage> = RwSignal::new_local(None);

    AnyView::new(view! {
        column(class = "gds-files-columns-body") {
            DialogHeader {
                DialogTitle {
                    row(class = "gds-files-columns-title") {
                        Icon(name = IconName::Columns3Cog) {}
                        text {{i18n::text("columns-title")}}
                    }
                }
            }
            row(class = "gds-files-columns-target-bar") {
                box(class = "gds-files-columns-folder-wrap") {
                    HintTooltip(text = full_address) {
                        row(class = "gds-files-columns-folder-badge") {
                            Icon(name = IconName::Folder, size = 14.0) {}
                            text(class = "gds-files-columns-folder-label") {{folder_name}}
                        }
                    }
                }
                row(class = "gds-files-columns-scope-group") {
                    text(class = "gds-files-columns-scope-label") {{i18n::text("columns-scope")}}
                    Select(
                        value = Binding::controlled(
                            scope_value,
                            move |next: String| {
                                scope.set(ColumnScope::from_value(&next));
                            }
                        )
                    ) {
                        SelectTrigger(
                            size = SelectTriggerSize::Sm,
                            label = i18n::text("columns-scope"),
                        ) {
                            SelectValue(placeholder = i18n::text("columns-scope"))
                        }
                        SelectContent {
                            SelectItem(value = "location") {{i18n::text("columns-scope-folder")}}
                            SelectItem(value = "default") {{i18n::text("columns-scope-global")}}
                        }
                    }
                }
            }
            column(class = "gds-files-columns-list") {
                {{move || {
                    let current = layout.get();
                    let visible_cols: Vec<Column> = current.columns().to_vec();
                    let len = visible_cols.len();
                    let min_movable =
                        usize::from(visible_cols.first().is_some_and(|c| c.is_required()));

                    let drag = ColumnDragState {
                        layout,
                        dragging_col,
                        drop_slot,
                        row_refs: Rc::new(
                            visible_cols
                                .iter()
                                .copied()
                                .filter(|c| !c.is_required())
                                .map(|c| (c, NodeRef::new()))
                                .collect(),
                        ),
                    };

                    let visible_views = visible_cols
                        .iter()
                        .copied()
                        .enumerate()
                        .map(|(index, column)| {
                            visible_column_row(column, index, len, min_movable, &drag)
                        })
                        .collect::<Vec<_>>();

                    let hidden_cols: Vec<Column> = Column::ALL
                        .into_iter()
                        .filter(|column| !current.contains(*column))
                        .collect();
                    let hidden_len = hidden_cols.len();
                    let hidden_views = hidden_cols
                        .into_iter()
                        .map(|column| hidden_column_row(column, layout))
                        .collect::<Vec<_>>();

                    view! {
                        column(class = "gds-files-columns-list") {
                            text(class = "gds-files-columns-group-heading") {
                                {i18n::count("columns-visible-group", len)}
                            }
                            {visible_views}
                            box(
                                class = "gds-files-columns-drop-end",
                                attr:data-drop-end = move || {
                                    (drop_slot.get() == Some(ColumnDropSlot::End))
                                        .then(|| "true".to_owned())
                                },
                            ) {}
                            {(hidden_len > 0).then(|| view! {
                                column(style:gap = "4px") {
                                    text(class = "gds-files-columns-group-heading") {
                                        {i18n::count("columns-hidden-group", hidden_len)}
                                    }
                                    {hidden_views}
                                }
                            })}
                        }
                    }
                }}}
            }
            DialogFooter {
                row(class = "gds-files-columns-footer") {
                    {move || {
                        let d_ctx = defaults.clone();
                        let loc = restore_loc.clone();
                        has_override.get().then(move || {
                            view! {
                                GdsButton(
                                    variant = GdsButtonVariant::Ghost,
                                    icon = IconName::RotateCcw,
                                    on:click = move |_| {
                                        apply_columns(
                                            &d_ctx,
                                            loc.clone(),
                                            None,
                                            ColumnScope::Location,
                                        );
                                    }
                                ) {
                                    {i18n::text("columns-restore")}
                                }
                            }
                            .into_view()
                        })
                    }}
                    box(class = "gds-files-columns-footer__right") {
                        DialogActions(
                            primary_label = i18n::text("columns-apply"),
                            cancel_label = i18n::text("dialog-cancel"),
                            on_decision = UnsyncCallback::new(move |decision: DialogDecision| {
                                match decision {
                                    DialogDecision::Confirm => apply_columns(
                                        &apply_ctx,
                                        apply.clone(),
                                        Some(layout.get_untracked()),
                                        scope.get_untracked(),
                                    ),
                                    DialogDecision::Cancel | DialogDecision::Dismiss => {
                                        confirm.0.menus.close();
                                    }
                                }
                            })
                        )
                    }
                }
            }
        }
    })
}

pub(super) fn apply_columns(
    ctx: &Context,
    location: Location,
    layout: Option<Layout>,
    scope: ColumnScope,
) {
    ctx.update(|state| {
        match (scope, layout) {
            (_, None) => state.places.clear_columns(location),
            (ColumnScope::Location, Some(mut layout)) => {
                let _ = layout.normalize();
                state.places.set_columns(location, layout);
            }
            (ColumnScope::Default, Some(mut layout)) => {
                let _ = layout.normalize();
                state.places.set_default_columns(layout);
                state.places.clear_columns(location);
            }
        }
        for tab in &mut state.tabs {
            tab.columns = state
                .places
                .effective_columns(tab.navigation.location())
                .clone();
            if !tab.columns.contains(tab.sort.key) {
                tab.sort = Default::default();
            }
        }
    });
    ctx.save_places();
    ctx.0.menus.close();
    let tabs = ctx.read_untracked(|state| state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>());
    let rows = Rows(ctx);
    let enrichment = EnrichmentRunner(ctx);
    for id in tabs {
        rows.reproject(id);
        enrichment.enrich_directory(id);
    }
}
