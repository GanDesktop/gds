use crate::entities::file::columns::{Column as LayoutColumn, Layout};
use crate::entities::location::{ColumnWidths, Location};
use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc, sync::Arc};

use fluent_bundle::FluentArgs;
use gds::ui::time::Timestamp;
use gds::{
    appearance::{
        tokens::{DataListStyle, TOOLTIP_DELAY},
        typography,
    },
    i18n,
};
use gds_uim::{el, m};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon,
    menu::PopupMenu,
    table::{Column, ColumnSort, TableDelegate, TableEvent, TableSelection, TableState},
    tooltip::Tooltip,
};
use gpui_kit::{prelude::*, *};

use super::{
    Browser,
    columns::sort_key,
    drag_drop::{self, DragPreview, DragSnapshot, FileDrag},
    entry_appearance::EntryAppearance,
    file_icon::FileIcon,
};
use crate::entities::file::directory::{ChildCount, ContentCategory, Entry, EntryKind, LinkStatus};
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
/// Shown for a metadata value the platform did not provide.
const UNKNOWN_VALUE: &str = "—";

pub(super) struct Data {
    pub rows: Arc<[Entry]>,
    pub selected: Vec<PathBuf>,
    pub sort: Sort,
    pub scroll: UniformListScrollHandle,
    pub content_width: f32,
    pub layout: Layout,
    pub location: Location,
    pub widths: ColumnWidths,
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
    fn new(entry: &Entry, selected: bool, cx: &App) -> Self {
        let icon = FileIcon::for_entry(entry);
        let foreground = if selected {
            cx.theme().accent_foreground
        } else {
            cx.theme().foreground
        };
        let icon_color = if selected {
            foreground
        } else if icon.base() == IconName::Folder {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        Self {
            name: entry.display_name.clone().into(),
            icon,
            appearance: EntryAppearance::of(entry, cx),
            foreground,
            icon_color,
            font_size: typography::scaled(DataListStyle::NAME_SIZE, cx),
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
                    $(self.icon.render(px(NAME_ICON_WIDTH), self.icon_color)),
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

pub(super) struct FileTable {
    browser: WeakEntity<Browser>,
    rows: Arc<[Entry]>,
    selected: Vec<PathBuf>,
    sort: Sort,
    sort_pending: bool,
    /// The layout these `columns` were built from.
    layout: Layout,
    columns: Vec<Column>,
    location: Option<Location>,
    menu_focus: Option<FocusHandle>,
    suppressed_hint: Option<PathBuf>,
    drag: DragSnapshot,
    name_bounds: Rc<RefCell<HashMap<usize, Bounds<Pixels>>>>,
    _native_selection: Option<Subscription>,
}

impl FileTable {
    /// Create the native table once; the caller retains this entity across renders.
    pub(super) fn make_state(
        browser: WeakEntity<Browser>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TableState<Self>> {
        let layout = Layout::default();
        let columns = Self::columns_for(&layout, 0., Sort::default(), cx);
        let state = cx.new(|cx| {
            TableState::new(
                Self {
                    browser,
                    rows: Arc::from([]),
                    selected: Vec::new(),
                    sort: Sort::default(),
                    sort_pending: false,
                    layout,
                    columns,
                    location: None,
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
    pub(super) fn sync_state(state: &Entity<TableState<Self>>, data: Data, cx: &mut App) {
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
        let mut columns = Self::columns_for(&layout, content_width.max(0.), sort, cx);
        for (column, column_ui) in layout.columns().iter().copied().zip(columns.iter_mut()) {
            if let Some(width) = widths.get(column.id()) {
                column_ui.width = px(*width);
            }
        }
        let scroll_changed = !Rc::ptr_eq(&state.read(cx).vertical_scroll_handle.0, &scroll.0);
        state.update(cx, |state, cx| {
            Self::clear_native_selection(state, cx);
            let delegate = state.delegate_mut();
            let refresh = delegate.location.as_ref() != Some(&location)
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
            let changed = !Arc::ptr_eq(&delegate.rows, &rows)
                || delegate.selected != selected
                || delegate.sort != sort;
            if changed {
                delegate.drag = DragSnapshot::new(&rows, &selected);
            }
            if changed || refresh {
                delegate.name_bounds.borrow_mut().clear();
            }
            delegate.rows = rows;
            delegate.selected = selected;
            delegate.sort = sort;
            delegate.sort_pending = false;
            delegate.layout = layout;
            delegate.columns = columns;
            delegate.location = Some(location);
            state.vertical_scroll_handle = scroll;
            if refresh {
                state.refresh(cx);
            }
            if changed || scroll_changed || refresh {
                cx.notify();
            }
        });
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

    fn render_name(
        &self,
        row_ix: usize,
        entry: &Entry,
        window: &Window,
        cx: &Context<TableState<Self>>,
    ) -> AnyElement {
        let selected = self.selected.contains(&entry.path);
        let content = NameContent::new(entry, selected, cx);
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
                # $("file-name", row_ix); flex; items_center; size_full; min_w_0; overflow_hidden;
                on_any_mouse_down: on_mouse_down; on_hover: on_hover;
                @if (hint_enabled) tooltip_show_delay: TOOLTIP_DELAY; tooltip: tooltip; @end;
            } [
                $(content.render().child(measure)),
            ]
        }
        .into_any_element()
    }

    fn render_meta(
        &self,
        entry: &Entry,
        text: impl IntoElement,
        emphasis: bool,
        right_aligned: bool,
        cx: &Context<TableState<Self>>,
    ) -> AnyElement {
        let appearance = EntryAppearance::of(entry, cx);
        let foreground = if self.selected.contains(&entry.path) {
            cx.theme().accent_foreground
        } else {
            cx.theme().muted_foreground
        };
        let weight = if emphasis {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        el! {
            .div { size_full; flex; items_center; min_w_0; overflow_hidden } [
                .div {
                    w_full; min_w_0; truncate;
                    text_size: $(typography::scaled(DataListStyle::META_SIZE, cx));
                    text_color: foreground;
                    font_weight: weight; opacity: $appearance.opacity;
                    @if (right_aligned) text_right; @end;
                } [text],
            ]
        }
        .into_any_element()
    }

    fn render_permissions(
        &self,
        row_ix: usize,
        entry: &Entry,
        cx: &Context<TableState<Self>>,
    ) -> AnyElement {
        let Some(permissions) = entry.details.permissions.as_ref() else {
            return self.render_meta(entry, SharedString::from(UNKNOWN_VALUE), false, false, cx);
        };
        let appearance = EntryAppearance::of(entry, cx);
        let text: SharedString = permissions.symbols().into();
        let octal = SharedString::from(permissions.octal());
        let foreground = if self.selected.contains(&entry.path) {
            cx.theme().accent_foreground
        } else {
            cx.theme().muted_foreground
        };
        let tooltip =
            move |window: &mut Window, cx: &mut App| Tooltip::new(octal.clone()).build(window, cx);
        el! {
            .div {
                # $("file-permissions", row_ix); size_full; flex; items_center; min_w_0; overflow_hidden;
                tooltip_show_delay: TOOLTIP_DELAY; tooltip: tooltip;
            } [
                .div {
                    w_full; min_w_0; truncate;
                    text_size: $(typography::scaled(DataListStyle::META_SIZE, cx)); text_color: foreground;
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
            crate::shared::lib::clipboard::refresh(cx);
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
                text_size: $(typography::scaled(DataListStyle::HEADER_SIZE, cx));
                font_weight: FontWeight::NORMAL; text_color: $(cx.theme().muted_foreground);
                when: $(column == LayoutColumn::Name), m! { pl: $(NAME_ICON_WIDTH + NAME_ICON_GAP).px };
            } [name]
        }
        .into_any_element()
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let row = el! { .div { # $("file-row", row_ix); } };
        let Some(entry) = self.rows.get(row_ix) else {
            return row;
        };
        let selected = self.selected.contains(&entry.path);
        let on_click = self.row_click_handler(row_ix, entry.path.clone());
        let drag = self.drag.drag_for(entry);
        let mut content = NameContent::new(entry, selected, cx);
        content.appearance = content.appearance.preview();
        let bounds = self.name_bounds.clone();
        bounds.borrow_mut().remove(&row_ix);
        let mut row = el! {
            row {
                relative; border_b_0; rounded: DataListStyle::ROW_RADIUS.px;
                cursor_pointer;
                on_click: on_click;
            } [
                @if (selected)
                    .div {
                        absolute; inset_0; top: DataListStyle::SELECTION_INSET.px; bottom: DataListStyle::SELECTION_INSET.px;
                        rounded: DataListStyle::ROW_RADIUS.px;
                        bg: $(cx.theme().selection);
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
                let content = content.clone();
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
        match column {
            LayoutColumn::Name => self.render_name(row_ix, entry, window, cx),
            LayoutColumn::Permissions => self.render_permissions(row_ix, entry, cx),
            LayoutColumn::Owner => {
                let owner = EntryPresentation::owner(entry, cx);
                self.render_meta(entry, owner.text, owner.current, false, cx)
            }
            LayoutColumn::Group => {
                let text = EntryPresentation::group(entry);
                self.render_meta(entry, text, false, false, cx)
            }
            LayoutColumn::Modified => self.render_meta(
                entry,
                Timestamp::new(("file-modified", row_ix), entry.modified),
                false,
                false,
                cx,
            ),
            LayoutColumn::Accessed => self.render_meta(
                entry,
                Timestamp::new(("file-accessed", row_ix), entry.details.accessed),
                false,
                false,
                cx,
            ),
            LayoutColumn::Type => {
                let text = EntryPresentation::type_label(entry, cx);
                self.render_meta(entry, text, false, false, cx)
            }
            LayoutColumn::DetailedType => {
                let text = EntryPresentation::detailed_type(entry, cx);
                self.render_meta(entry, text, false, false, cx)
            }
            LayoutColumn::Size => {
                let text = EntryPresentation::size_label(entry, cx);
                self.render_meta(entry, text, false, true, cx)
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

/// Formats the non-icon cells of a file row; the details view shares these.
pub(super) struct EntryPresentation;

/// Owner cell contents; the current user's name is emphasized.
pub(super) struct OwnerLabel {
    pub(super) text: SharedString,
    pub(super) current: bool,
}

impl EntryPresentation {
    pub(super) fn name_hint(name: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView {
        move |window, cx| Tooltip::new(name.clone()).build(window, cx)
    }

    pub(super) fn owner(entry: &Entry, cx: &App) -> OwnerLabel {
        let Some(owner) = entry.details.owner.as_ref() else {
            return OwnerLabel {
                text: UNKNOWN_VALUE.into(),
                current: false,
            };
        };
        let name = owner.label().into_owned();
        if !owner.is_current_user {
            return OwnerLabel {
                text: name.into(),
                current: false,
            };
        }
        let mut arguments = FluentArgs::new();
        arguments.set("name", name);
        OwnerLabel {
            text: i18n::format("owner-you", Some(&arguments), cx),
            current: true,
        }
    }

    pub(super) fn group(entry: &Entry) -> SharedString {
        entry.details.group.as_ref().map_or_else(
            || UNKNOWN_VALUE.into(),
            |group| group.label().into_owned().into(),
        )
    }

    /// Localized broad type, or the link relationship for a symbolic link.
    pub(super) fn type_label(entry: &Entry, cx: &App) -> SharedString {
        if entry.kind == EntryKind::Directory {
            return i18n::text("kind-directory", cx);
        }
        if entry.kind != EntryKind::SymbolicLink {
            return Self::kind_label(entry, cx);
        }
        let Some(target) = entry.details.target.as_ref() else {
            return i18n::text("kind-link", cx);
        };
        match target.status {
            LinkStatus::Broken => i18n::text("link-broken", cx),
            LinkStatus::Unreachable => i18n::text("link-unreachable", cx),
            LinkStatus::Resolved => {
                let target_label: SharedString = match target.kind {
                    EntryKind::Directory => i18n::text("kind-directory", cx),
                    EntryKind::File => entry.details.display_content().map_or_else(
                        || i18n::text("kind-file", cx),
                        |content| Self::category_label(content.category, cx),
                    ),
                    EntryKind::Other => i18n::text("kind-other", cx),
                    EntryKind::Unknown => i18n::text("kind-unknown", cx),
                    EntryKind::SymbolicLink => i18n::text("kind-link", cx),
                };
                let mut arguments = FluentArgs::new();
                arguments.set("target", target_label.to_string());
                i18n::format("type-link", Some(&arguments), cx)
            }
        }
    }

    /// Concrete format name, MIME fallback, or an explicit unknown value.
    pub(super) fn detailed_type(entry: &Entry, cx: &App) -> SharedString {
        if entry.kind == EntryKind::Directory {
            return i18n::text("kind-directory", cx);
        }
        let Some(content) = entry.details.display_content() else {
            return UNKNOWN_VALUE.into();
        };
        match content.label() {
            Some(label) => label.into(),
            None => i18n::text("kind-unknown", cx),
        }
    }

    fn kind_label(entry: &Entry, cx: &App) -> SharedString {
        match entry.kind {
            EntryKind::Directory => i18n::text("kind-directory", cx),
            EntryKind::File => entry.details.display_content().map_or_else(
                || i18n::text("kind-file", cx),
                |content| Self::category_label(content.category, cx),
            ),
            EntryKind::SymbolicLink => i18n::text("kind-link", cx),
            EntryKind::Other => i18n::text("kind-other", cx),
            EntryKind::Unknown => i18n::text("kind-unknown", cx),
        }
    }

    fn category_label(category: ContentCategory, cx: &App) -> SharedString {
        let id = match category {
            ContentCategory::Text => "content-text",
            ContentCategory::Code => "content-code",
            ContentCategory::Document => "content-document",
            ContentCategory::Image => "content-image",
            ContentCategory::Audio => "content-audio",
            ContentCategory::Video => "content-video",
            ContentCategory::Archive => "content-archive",
            ContentCategory::Executable => "content-executable",
            ContentCategory::Font => "content-font",
            ContentCategory::Binary => "content-binary",
        };
        i18n::text(id, cx)
    }

    /// Byte size for the details panel; a directory has no byte size there.
    pub(super) fn size(entry: &Entry) -> String {
        if entry.kind == EntryKind::Directory {
            UNKNOWN_VALUE.to_owned()
        } else {
            entry
                .size
                .map(Self::format_size)
                .unwrap_or_else(|| UNKNOWN_VALUE.into())
        }
    }

    /// Size cell contents: bytes for a file, the direct child count for a
    /// directory. A directory whose children could not be counted stays unknown.
    pub(super) fn size_label(entry: &Entry, cx: &App) -> SharedString {
        if entry.kind == EntryKind::Directory {
            return match entry.details.children {
                Some(children) => Self::child_count_label(children, cx),
                None => UNKNOWN_VALUE.into(),
            };
        }
        entry.size.map_or_else(
            || UNKNOWN_VALUE.into(),
            |bytes| Self::format_size(bytes).into(),
        )
    }

    /// A count that stopped at the read limit is shown as a lower bound so the
    /// cell never claims a precision it does not have.
    fn child_count_label(children: ChildCount, cx: &App) -> SharedString {
        let id = if children.truncated {
            "directory-item-count-at-least"
        } else {
            "directory-item-count"
        };
        i18n::count(id, children.count as usize, cx)
    }

    pub(super) fn format_size(bytes: u64) -> String {
        const KIB: f64 = 1024.;
        let bytes = bytes as f64;
        if bytes >= KIB * KIB * KIB {
            format!("{:.1} GiB", bytes / (KIB * KIB * KIB))
        } else if bytes >= KIB * KIB {
            format!("{:.1} MiB", bytes / (KIB * KIB))
        } else if bytes >= KIB {
            format!("{:.1} KiB", bytes / KIB)
        } else {
            format!("{bytes:.0} B")
        }
    }
}
