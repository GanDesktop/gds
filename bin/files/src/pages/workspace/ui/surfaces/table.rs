use crate::entities::file::columns::{Column as LayoutColumn, Layout};
use crate::entities::location::{ColumnWidths, Location};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use gds::ui::time::Timestamp;
use gds::{
    app::AppExt as _,
    appearance::tokens::{DataListStyle, TOOLTIP_DELAY},
    i18n,
};
use gds_uim::{el, m};
use gpui_kit::assets::IconName;
use gpui_kit::base::motion::Interpolate as _;
use gpui_kit::component::{
    ActiveTheme, Icon,
    menu::PopupMenu,
    table::{Column, ColumnSort, TableDelegate, TableEvent, TableSelection, TableState},
    tooltip::Tooltip,
};
use gpui_kit::{prelude::*, *};

use crate::entities::file::ui::{EntryPresentation, FileArtwork, FileIcon, UNKNOWN_VALUE};

use super::{
    Browser, FilesExt as _, Selection,
    columns::sort_key,
    drag_drop::{self, DragPreview, DragSnapshot, FileDrag},
    entry_appearance::EntryAppearance,
};
use crate::entities::file::directory::{Entry, EntryKind};
use crate::entities::file::sorting::{Direction, Key, Sort};

const NAME_MIN_WIDTH: f32 = 180.;
const NAME_DEFAULT_MAX_WIDTH: f32 = 500.;
const NAME_WIDTH_FRACTION: f32 = 0.45;
const MODIFIED_WIDTH: f32 = 160.;
const KIND_WIDTH: f32 = 110.;
const SIZE_WIDTH: f32 = 100.;
const OWNER_WIDTH: f32 = 130.;
const GROUP_WIDTH: f32 = 130.;
const PERMISSIONS_WIDTH: f32 = 96.;
const ACCESSED_WIDTH: f32 = 160.;
const DETAILED_TYPE_WIDTH: f32 = 130.;
const MODIFIED_MIN_WIDTH: f32 = 144.;
const KIND_MIN_WIDTH: f32 = 96.;
const SIZE_MIN_WIDTH: f32 = 88.;
const OWNER_MIN_WIDTH: f32 = 90.;
const GROUP_MIN_WIDTH: f32 = 90.;
const PERMISSIONS_MIN_WIDTH: f32 = 84.;
const ACCESSED_MIN_WIDTH: f32 = 144.;
const DETAILED_TYPE_MIN_WIDTH: f32 = 90.;
const NAME_ICON_WIDTH: f32 = 18.;
const NAME_ICON_GAP: f32 = 10.;
const CUT_MARKER_SIZE: f32 = 12.;
const CUT_MARKER_GAP: f32 = 6.;

pub(in crate::pages::workspace) struct Data {
    pub rows: Arc<[Entry]>,
    pub selected: Selection<PathBuf>,
    pub sort: Sort,
    pub scroll: UniformListScrollHandle,
    pub content_width: f32,
    pub layout: Layout,
    pub location: Location,
    pub widths: ColumnWidths,
}

#[derive(Clone, Copy)]
struct RowCell<'a> {
    row_ix: usize,
    entry: &'a Entry,
    selection: f32,
    emphasis: bool,
    right_aligned: bool,
}

impl<'a> RowCell<'a> {
    fn new(row_ix: usize, entry: &'a Entry, selection: f32) -> Self {
        Self {
            row_ix,
            entry,
            selection,
            emphasis: false,
            right_aligned: false,
        }
    }

    fn emphasis(mut self, emphasis: bool) -> Self {
        self.emphasis = emphasis;
        self
    }

    fn right_aligned(mut self) -> Self {
        self.right_aligned = true;
        self
    }
}

/// The name cell and its floating drag use the same content and typography.
#[derive(Clone)]
struct NameContent {
    name: SharedString,
    icon: FileIcon,
    appearance: EntryAppearance,
    foreground: Hsla,
    icon_color: Hsla,
    font_size: Pixels,
}

impl NameContent {
    fn new(entry: &Entry, selection: f32, cx: &App) -> Self {
        let icon = FileIcon::for_entry(entry);
        let foreground = cx
            .theme()
            .foreground
            .interpolate(&cx.theme().accent_foreground, selection);
        let icon_color =
            FileIcon::color(entry, cx).interpolate(&cx.theme().accent_foreground, selection);
        Self {
            name: entry.display_name.clone().into(),
            icon,
            appearance: EntryAppearance::of(entry, cx),
            foreground,
            icon_color,
            font_size: cx
                .gds_ro()
                .appearance()
                .typography()
                .scaled(DataListStyle::NAME_SIZE),
        }
    }

    fn render(&self) -> Div {
        el! {
            .div {
                relative; flex; items_center; min_w_0; max_w_full;
                text_size: $self.font_size; text_color: $self.foreground;
            } [
                .div {
                    flex; items_center; min_w_0; gap: NAME_ICON_GAP.px;
                    opacity: $self.appearance.opacity;
                } [
                    .FileArtwork::default { icon: $(self.icon); size: NAME_ICON_WIDTH.px; color: $(self.icon_color); },
                    .div { min_w_0; truncate } [$(self.name.clone())],
                ],
                @if (self.appearance.cut)
                    .div { flex_shrink_0; ml: CUT_MARKER_GAP.px } [
                        IconName::Scissors.Icon::new {
                            size: CUT_MARKER_SIZE.px; text_color: $self.foreground;
                        },
                    ],
                @end,
            ]
        }
    }
}

pub(in crate::pages::workspace) struct FileTable {
    browser: WeakEntity<Browser>,
    rows: Arc<[Entry]>,
    selected: Selection<PathBuf>,
    row_selection: Option<(usize, f32)>,
    sort: Sort,
    sort_pending: bool,
    enrich_pending: bool,
    /// The layout these `columns` were built from.
    layout: Layout,
    columns: Vec<Column>,
    location: Option<Location>,
    content_width: f32,
    widths: ColumnWidths,
    language: i18n::Language,
    menu_focus: Option<FocusHandle>,
    suppressed_hint: Option<PathBuf>,
    drag: DragSnapshot,
    name_bounds: Rc<RefCell<HashMap<usize, Bounds<Pixels>>>>,
    _native_selection: Option<Subscription>,
}

impl FileTable {
    /// Create the native table once; the caller retains this entity across renders.
    pub(in crate::pages::workspace) fn make_state(
        browser: WeakEntity<Browser>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TableState<Self>> {
        let layout = Layout::default();
        let columns = Self::columns_for(&layout, 0., Sort::default(), cx);
        let language = cx.gds_ro().i18n().language();
        let state = cx.new(|cx| {
            TableState::new(
                Self {
                    browser,
                    rows: Arc::from([]),
                    selected: Selection::new(),
                    row_selection: None,
                    sort: Sort::default(),
                    sort_pending: false,
                    enrich_pending: false,
                    layout,
                    columns,
                    location: None,
                    content_width: 0.,
                    widths: ColumnWidths::default(),
                    language,
                    menu_focus: None,
                    suppressed_hint: None,
                    drag: DragSnapshot::default(),
                    name_bounds: Rc::default(),
                    _native_selection: None,
                },
                window,
                cx,
            )
            // Files owns its path-based Ctrl/Shift selection. The native table
            // must not maintain a competing single-row selection.
            .row_selectable(false)
            .col_selectable(false)
            .cell_selectable(false)
            .col_movable(false)
            .col_resizable(true)
            .sortable(true)
        });
        let subscription = cx.subscribe(&state, Self::on_table_event);
        state.update(cx, |state, _| {
            state.delegate_mut()._native_selection = Some(subscription)
        });
        state
    }

    fn on_table_event(state: Entity<TableState<Self>>, event: &TableEvent, cx: &mut App) {
        if let TableEvent::ColumnWidthsChanged(widths) = event {
            let delegate = state.read(cx).delegate();
            let Some(location) = delegate.location.clone() else {
                return;
            };
            let changed: ColumnWidths = delegate
                .layout
                .columns()
                .iter()
                .zip(&delegate.columns)
                .zip(widths)
                .filter(|((_, column), width)| column.width != **width)
                .map(|((column, _), width)| (column.id().to_owned(), f32::from(*width)))
                .collect();
            if changed.is_empty() {
                return;
            }
            let browser = delegate.browser.clone();
            cx.defer(move |cx| {
                let _ = browser.update(cx, |browser, cx| {
                    browser.locations.places.set_widths(location, changed);
                    browser.save_places(cx);
                    cx.notify();
                });
            });
            return;
        }
        if matches!(
            event,
            TableEvent::SelectRow(_) | TableEvent::SelectColumn(_) | TableEvent::SelectCell(_, _)
        ) {
            state.update(cx, Self::clear_native_selection);
        }
    }

    fn clear_native_selection(state: &mut TableState<Self>, cx: &mut Context<TableState<Self>>) {
        // Native key actions can set a selection even when selection flags are disabled.
        // Browser paths remain the only selection model for Files.
        if !matches!(state.selection(), TableSelection::None) {
            state.clear_selection(cx);
        }
    }

    fn clear_menu_target(
        state: &mut TableState<Self>,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        if state.right_clicked_row().is_some() {
            state.set_right_clicked_row(None, cx);
        }
        Self::clear_native_selection(state, cx);
    }

    /// Project the location's saved widths by column identity, independent of
    /// the order or visibility of the current layout.
    pub(in crate::pages::workspace) fn sync_state(
        state: &Entity<TableState<Self>>,
        data: Data,
        cx: &mut App,
    ) {
        let Data {
            rows,
            selected,
            sort,
            scroll,
            content_width,
            layout,
            location,
            widths,
        } = data;
        let content_width = content_width.max(0.);
        let language = cx.gds_ro().i18n().language();
        let (scroll_changed, columns_dirty) = {
            let table = state.read(cx);
            let delegate = table.delegate();
            let scroll_changed = !Rc::ptr_eq(&table.vertical_scroll_handle.0, &scroll.0);
            let columns_dirty = delegate.location.as_ref() != Some(&location)
                || delegate.sort_pending
                || delegate.sort != sort
                || delegate.layout != layout
                || (delegate.content_width - content_width).abs() > f32::EPSILON
                || delegate.widths != widths
                || delegate.language != language;
            (scroll_changed, columns_dirty)
        };
        let next_columns = columns_dirty.then(|| {
            let mut columns = Self::columns_for(&layout, content_width, sort, cx);
            for (column, column_ui) in layout.columns().iter().copied().zip(columns.iter_mut()) {
                if let Some(width) = widths.get(column.id()) {
                    column_ui.width = px(*width);
                }
            }
            columns
        });
        state.update(cx, |state, cx| {
            Self::clear_native_selection(state, cx);
            let delegate = state.delegate_mut();
            let refresh = if let Some(columns) = next_columns {
                let needs_refresh = delegate.location.as_ref() != Some(&location)
                    || delegate.sort_pending
                    || delegate.layout != layout
                    || delegate.columns.len() != columns.len()
                    || columns
                        .iter()
                        .zip(delegate.columns.iter())
                        .any(|(next, current)| {
                            next.key != current.key
                                || next.name != current.name
                                || next.width != current.width
                                || next.sort != current.sort
                        });
                delegate.layout = layout;
                delegate.columns = columns;
                delegate.location = Some(location);
                delegate.content_width = content_width;
                delegate.widths = widths;
                delegate.language = language;
                needs_refresh
            } else {
                false
            };
            let rows_changed = !Arc::ptr_eq(&delegate.rows, &rows);
            let selection_changed = delegate.selected != selected;
            let sort_changed = delegate.sort != sort;
            let changed = rows_changed || selection_changed || sort_changed;
            if changed {
                delegate.drag = DragSnapshot::new(&rows, &selected);
                delegate.row_selection = None;
            }
            if rows_changed || sort_changed || refresh {
                delegate.name_bounds.borrow_mut().clear();
            }
            delegate.rows = rows;
            delegate.selected = selected;
            delegate.sort = sort;
            delegate.sort_pending = false;
            state.vertical_scroll_handle = scroll;
            if refresh {
                state.refresh(cx);
            }
            if changed || scroll_changed || refresh {
                cx.notify();
            }
        });
    }

    pub(in crate::pages::workspace) fn update_rows(
        state: &Entity<TableState<Self>>,
        rows: Arc<[Entry]>,
        notify: bool,
        cx: &mut App,
    ) {
        state.update(cx, |state, cx| {
            let delegate = state.delegate_mut();
            delegate.drag = DragSnapshot::new(&rows, &delegate.selected);
            delegate.row_selection = None;
            delegate.rows = rows;
            if notify {
                cx.notify();
            }
        });
    }

    fn is_selected(&self, path: &Path) -> bool {
        self.selected.contains(path)
    }

    fn columns_for(layout: &Layout, content_width: f32, sort: Sort, cx: &App) -> Vec<Column> {
        let name_width =
            (content_width * NAME_WIDTH_FRACTION).clamp(NAME_MIN_WIDTH, NAME_DEFAULT_MAX_WIDTH);
        layout
            .columns()
            .iter()
            .copied()
            .map(|layout_column| {
                let (width, min_width) = match layout_column {
                    LayoutColumn::Name => (name_width, NAME_MIN_WIDTH),
                    LayoutColumn::Type => (KIND_WIDTH, KIND_MIN_WIDTH),
                    LayoutColumn::Size => (SIZE_WIDTH, SIZE_MIN_WIDTH),
                    LayoutColumn::Owner => (OWNER_WIDTH, OWNER_MIN_WIDTH),
                    LayoutColumn::Group => (GROUP_WIDTH, GROUP_MIN_WIDTH),
                    LayoutColumn::Permissions => (PERMISSIONS_WIDTH, PERMISSIONS_MIN_WIDTH),
                    LayoutColumn::Modified => (MODIFIED_WIDTH, MODIFIED_MIN_WIDTH),
                    LayoutColumn::Accessed => (ACCESSED_WIDTH, ACCESSED_MIN_WIDTH),
                    LayoutColumn::DetailedType => (DETAILED_TYPE_WIDTH, DETAILED_TYPE_MIN_WIDTH),
                };
                let sort_icon = if sort_key(layout_column) == sort.key {
                    match sort.direction {
                        Direction::Ascending => ColumnSort::Ascending,
                        Direction::Descending => ColumnSort::Descending,
                    }
                } else {
                    ColumnSort::Default
                };
                Column::new(layout_column.id(), i18n::text(layout_column.label_id(), cx))
                    .width(px(width))
                    .min_width(px(min_width))
                    .paddings(Edges {
                        top: px(DataListStyle::CELL_PADDING_Y),
                        bottom: px(DataListStyle::CELL_PADDING_Y),
                        left: px(DataListStyle::CELL_PADDING_X),
                        right: px(DataListStyle::CELL_PADDING_X),
                    })
                    .resizable(true)
                    .movable(false)
                    .sort(sort_icon)
            })
            .collect()
    }

    /// Request ordering by `key`, reversing the direction when it is already active.
    fn request_sort(&mut self, key: Key, cx: &mut App) {
        let sort = self.sort.next(key);
        self.sort = sort;
        self.sort_pending = true;
        if let Some(browser) = self.browser.upgrade() {
            browser.update(cx, |browser, cx| browser.set_sort(sort, cx));
        }
    }

    fn header_click_handler(
        table: WeakEntity<TableState<Self>>,
        key: Key,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, _, cx| {
            cx.stop_propagation();
            if let Some(table) = table.upgrade() {
                table.update(cx, |table, cx| table.delegate_mut().request_sort(key, cx));
            }
        }
    }

    fn on_name_mouse_down(
        path: PathBuf,
    ) -> impl Fn(&mut TableState<Self>, &MouseDownEvent, &mut Window, &mut Context<TableState<Self>>)
    {
        move |table, _, _, cx| {
            let delegate = table.delegate_mut();
            if delegate.suppressed_hint.as_ref() != Some(&path) {
                delegate.suppressed_hint = Some(path.clone());
                cx.notify();
            }
        }
    }

    fn on_name_hover(
        path: PathBuf,
    ) -> impl Fn(&mut TableState<Self>, &bool, &mut Window, &mut Context<TableState<Self>>) {
        move |table, hovered, _, cx| {
            let delegate = table.delegate_mut();
            if !hovered && delegate.suppressed_hint.as_ref() == Some(&path) {
                delegate.suppressed_hint = None;
                cx.notify();
            }
        }
    }

    fn selection_progress(
        &self,
        entry: &Entry,
        selected: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> f32 {
        super::motion::state_progress(
            format!("file-row-selection:{}", entry.path.display()),
            selected,
            window,
            cx,
        )
    }

    fn row_selection_for(
        &self,
        row_ix: usize,
        entry: &Entry,
        window: &mut Window,
        cx: &mut App,
    ) -> f32 {
        self.row_selection
            .filter(|(cached_row, _)| *cached_row == row_ix)
            .map_or_else(
                || self.selection_progress(entry, self.is_selected(&entry.path), window, cx),
                |(_, progress)| progress,
            )
    }

    fn render_name(
        &self,
        cell: RowCell<'_>,
        window: &Window,
        cx: &Context<TableState<Self>>,
    ) -> AnyElement {
        let RowCell {
            row_ix,
            entry,
            selection,
            ..
        } = cell;
        let content = NameContent::new(entry, selection, cx);
        let tooltip = EntryPresentation::name_hint(content.name.clone());
        let hint_enabled = self.suppressed_hint.as_ref() != Some(&entry.path)
            && self
                .menu_focus
                .as_ref()
                .is_none_or(|focus| !focus.contains_focused(window, cx));
        let on_mouse_down = cx.listener(Self::on_name_mouse_down(entry.path.clone()));
        let on_hover = cx.listener(Self::on_name_hover(entry.path.clone()));
        let bounds = self.name_bounds.clone();
        let measure = canvas(
            move |measured, _, _| {
                bounds.borrow_mut().insert(row_ix, measured);
            },
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
        el! {
            .div {
                # $("file-name", row_ix); test_support; flex; items_center; size_full; min_w_0; overflow_hidden;
                on_any_mouse_down: on_mouse_down; on_hover: on_hover;
                @if (hint_enabled) tooltip_show_delay: TOOLTIP_DELAY; tooltip: tooltip; @end;
            } [
                $(content.render()) [measure],
            ]
        }
        .into_any_element()
    }

    fn render_meta(
        &self,
        cell: RowCell<'_>,
        text: impl IntoElement,
        cx: &Context<TableState<Self>>,
    ) -> AnyElement {
        let appearance = EntryAppearance::of(cell.entry, cx);
        let foreground = cx
            .theme()
            .muted_foreground
            .interpolate(&cx.theme().accent_foreground, cell.selection);
        let weight = if cell.emphasis {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        el! {
            .div { size_full; flex; items_center; min_w_0; overflow_hidden } [
                .div {
                    w_full; min_w_0; truncate;
                    text_size: $(cx.gds_ro().appearance().typography().scaled(DataListStyle::META_SIZE));
                    text_color: foreground;
                    font_weight: weight; opacity: $appearance.opacity;
                    @if (cell.right_aligned) text_right; @end;
                } [text],
            ]
        }
        .into_any_element()
    }

    fn render_permissions(&self, cell: RowCell<'_>, cx: &Context<TableState<Self>>) -> AnyElement {
        let Some(permissions) = cell.entry.details.permissions.as_ref() else {
            return self.render_meta(cell, SharedString::from(UNKNOWN_VALUE), cx);
        };
        let appearance = EntryAppearance::of(cell.entry, cx);
        let text: SharedString = permissions.symbols().into();
        let octal = SharedString::from(permissions.octal());
        let foreground = cx
            .theme()
            .muted_foreground
            .interpolate(&cx.theme().accent_foreground, cell.selection);
        let tooltip =
            move |window: &mut Window, cx: &mut App| Tooltip::new(octal.clone()).build(window, cx);
        el! {
            .div {
                # $("file-permissions", cell.row_ix); size_full; flex; items_center; min_w_0; overflow_hidden;
                tooltip_show_delay: TOOLTIP_DELAY; tooltip: tooltip;
            } [
                .div {
                    w_full; min_w_0; truncate;
                    text_size: $(cx.gds_ro().appearance().typography().scaled(DataListStyle::META_SIZE)); text_color: foreground;
                    opacity: $appearance.opacity;
                } [text],
            ]
        }
        .into_any_element()
    }

    fn row_click_handler(
        &self,
        row_ix: usize,
        path: PathBuf,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let browser = self.browser.clone();
        move |event, window, cx| {
            // Text copied in this application's own inputs silently replaces the
            // file clipboard; revalidate the cut highlight on file interaction.
            cx.gds().files().clipboard().refresh();
            if let Some(browser) = browser.upgrade() {
                browser.update(cx, |browser, cx| {
                    browser.select(row_ix, event.modifiers(), cx);
                    browser.window.focus.focus(window, cx);
                    if event.click_count() == 2 {
                        browser.activate_path(path.clone(), cx);
                    }
                });
            }
        }
    }
}

impl TableDelegate for FileTable {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        _: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        if let Some(column) = self.layout.columns().get(col_ix).copied() {
            // Files always has one active ordering; Kit's unsorted stage is not a model state.
            self.request_sort(sort_key(column), cx);
        }
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let name = self.columns[col_ix].name.clone();
        let Some(column) = self.layout.columns().get(col_ix).copied() else {
            return el! { .div { size_full } [name] }.into_any_element();
        };
        let on_click = Self::header_click_handler(cx.entity().downgrade(), sort_key(column));
        el! {
            .div {
                # $("file-column", col_ix);
                size_full; flex; items_center; cursor_pointer; on_click: on_click;
                text_size: $(cx.gds_ro().appearance().typography().scaled(DataListStyle::HEADER_SIZE));
                font_weight: FontWeight::NORMAL; text_color: $(cx.theme().muted_foreground);
                when: $(column == LayoutColumn::Name), m! { pl: $(NAME_ICON_WIDTH + NAME_ICON_GAP).px };
            } [name]
        }
        .into_any_element()
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let row = el! { .div { # $("file-row", row_ix); } };
        let Some(entry) = self.rows.get(row_ix) else {
            return row;
        };
        let need = crate::entities::file::directory::EnrichmentNeed {
            children: self.layout.columns().contains(&LayoutColumn::Size)
                || self.sort.key == Key::Size,
            content: true,
            symlinks: true,
            full_budget: false,
        };
        if !self.enrich_pending && entry.needs_enrichment(need) {
            self.enrich_pending = true;
            cx.defer_in(window, |state, _, cx| {
                let delegate = state.delegate_mut();
                delegate.enrich_pending = false;
                if let Some(browser) = delegate.browser.upgrade() {
                    browser.update(cx, |browser, cx| {
                        browser.schedule_enrichment(browser.tabs.active_id, cx);
                    });
                }
            });
        }
        let selected = self.is_selected(&entry.path);
        let selection = self.selection_progress(entry, selected, window, cx);
        self.row_selection = Some((row_ix, selection));
        let on_click = self.row_click_handler(row_ix, entry.path.clone());
        let drag = self.drag.drag_for_row(entry, selected);
        let drag_entry = entry.clone();
        let bounds = self.name_bounds.clone();
        bounds.borrow_mut().remove(&row_ix);
        let mut row = el! {
            row {
                relative; border_b_0; rounded: DataListStyle::ROW_RADIUS.px;
                cursor_pointer;
                on_click: on_click;
            } [
                @if (selection > 0.)
                    .div {
                        absolute; inset_0; top: DataListStyle::SELECTION_INSET.px; bottom: DataListStyle::SELECTION_INSET.px;
                        rounded: DataListStyle::ROW_RADIUS.px;
                        bg: $(cx.theme().selection.opacity(selection));
                    },
                @end,
            ]
        };
        // The drag threshold suppresses the row click, so starting a drag never
        // changes the selection or opens the entry.
        row = row
            .on_drag(drag, move |drag, cursor_offset, window, cx| {
                let bounds = bounds.borrow().get(&row_ix).copied().unwrap_or_else(|| {
                    Bounds::new(
                        window.mouse_position(),
                        size(px(NAME_MIN_WIDTH), px(DataListStyle::ROW_HEIGHT)),
                    )
                });
                let offset =
                    DragPreview::source_offset(bounds, cursor_offset, window.mouse_position());
                let mut content = NameContent::new(&drag_entry, if selected { 1. } else { 0. }, cx);
                content.appearance = content.appearance.preview();
                DragPreview::new(
                    drag,
                    cursor_offset,
                    move |_, _| {
                        el! {
                            .div {
                                relative; left: $offset.x; top: $offset.y;
                                w: $bounds.size.width; h: $bounds.size.height;
                                flex; items_center;
                            } [$(content.render())]
                        }
                        .into_any_element()
                    },
                    cx,
                )
            })
            .external_drag_payload::<FileDrag>(|drag, _, _| Some(drag.external_payload()));
        if entry.kind == EntryKind::Directory {
            row = drag_drop::drop_target(row, entry.path.clone(), self.browser.clone());
        }
        row
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(entry) = self.rows.get(row_ix) else {
            return el! { .div }.into_any_element();
        };
        let Some(column) = self.layout.columns().get(col_ix).copied() else {
            return el! { .div }.into_any_element();
        };
        let selection = self.row_selection_for(row_ix, entry, window, cx);
        let cell = RowCell::new(row_ix, entry, selection);
        match column {
            LayoutColumn::Name => self.render_name(cell, window, cx),
            LayoutColumn::Permissions => self.render_permissions(cell, cx),
            LayoutColumn::Owner => {
                let owner = EntryPresentation::owner(entry, cx);
                self.render_meta(cell.emphasis(owner.current), owner.text, cx)
            }
            LayoutColumn::Group => {
                let text = EntryPresentation::group(entry);
                self.render_meta(cell, text, cx)
            }
            LayoutColumn::Modified => self.render_meta(
                cell,
                Timestamp::new(("file-modified", row_ix), entry.modified),
                cx,
            ),
            LayoutColumn::Accessed => self.render_meta(
                cell,
                Timestamp::new(("file-accessed", row_ix), entry.details.accessed),
                cx,
            ),
            LayoutColumn::Type => {
                let text = EntryPresentation::type_label(entry, cx);
                self.render_meta(cell, text, cx)
            }
            LayoutColumn::DetailedType => {
                let text = EntryPresentation::detailed_type(entry, cx);
                self.render_meta(cell, text, cx)
            }
            LayoutColumn::Size => {
                let text = EntryPresentation::size_label(entry, cx);
                self.render_meta(cell.right_aligned(), text, cx)
            }
        }
    }

    fn render_last_empty_col(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        el! { .div { w: 0.0.px; h_full; flex_shrink_0; } }
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let path = self.rows.get(row_ix).map(|entry| entry.path.clone());
        // The popup owns its captured target; clear Kit's visual target after this borrow ends.
        cx.defer_in(window, Self::clear_menu_target);
        let Some(path) = path else {
            return menu;
        };
        let Some(browser) = self.browser.upgrade() else {
            return menu;
        };
        // Omitting the name tooltip builder drops both visible and waiting GPUI hints.
        self.menu_focus = Some(menu.focus_handle(cx));
        cx.notify();
        browser.update(cx, |browser, cx| {
            browser.entry_context_menu(path, menu, window, cx)
        })
    }
}
