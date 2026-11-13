//! Kit table projection for trashed records.
//!
//! Rows are the backend listing in its own order; nothing here sorts by
//! original path or merges records that share one. Selection is the browser's
//! id set, so the native table keeps its own selection disabled.

use std::{collections::HashSet, ffi::OsString, sync::Arc};

use gds::{
    app::AppExt as _,
    appearance::tokens::{DataListStyle, TOOLTIP_DELAY},
    i18n,
    ui::time::Timestamp,
};
use gds_uim::{el, m};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon,
    menu::PopupMenu,
    table::{Column, TableDelegate, TableEvent, TableSelection, TableState},
    tooltip::Tooltip,
};
use gpui_kit::{prelude::*, *};

use super::backend::Entry;
use crate::entities::file::{
    directory::EntryKind,
    ui::{EntryPresentation, UNKNOWN_VALUE},
};
use crate::pages::workspace::ui::{Browser, Collection};

const NAME_FRACTION: f32 = 0.38;
const NAME_MIN_WIDTH: f32 = 200.;
const NAME_MAX_WIDTH: f32 = 520.;
const NAME_ICON_WIDTH: f32 = 18.;
const NAME_ICON_GAP: f32 = 10.;
const ORIGINAL_WIDTH: f32 = 320.;
const ORIGINAL_MIN_WIDTH: f32 = 160.;
const DELETED_WIDTH: f32 = 160.;
const DELETED_MIN_WIDTH: f32 = 140.;
const SIZE_WIDTH: f32 = 120.;
const SIZE_MIN_WIDTH: f32 = 90.;

pub(in crate::pages::workspace) struct Data {
    pub rows: Arc<[Entry]>,
    pub selected: HashSet<OsString>,
    pub content_width: f32,
}

pub(in crate::pages::workspace) struct TrashTable {
    browser: WeakEntity<Browser>,
    rows: Arc<[Entry]>,
    selected: HashSet<OsString>,
    columns: Vec<Column>,
    menu_focus: Option<FocusHandle>,
    _native_selection: Option<Subscription>,
}

impl TrashTable {
    /// Create the native table once; the caller retains this entity across renders.
    pub(in crate::pages::workspace) fn make_state(
        browser: WeakEntity<Browser>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TableState<Self>> {
        let columns = Self::columns_for(0., cx);
        let state = cx.new(|cx| {
            TableState::new(
                Self {
                    browser,
                    rows: Arc::from([]),
                    selected: HashSet::new(),
                    columns,
                    menu_focus: None,
                    _native_selection: None,
                },
                window,
                cx,
            )
            // Trash selection is the browser's id set; the native table must
            // not maintain a competing row selection.
            .row_selectable(false)
            .col_selectable(false)
            .cell_selectable(false)
            .col_movable(false)
            .col_resizable(false)
            .sortable(false)
        });
        let subscription = cx.subscribe(&state, Self::on_native_selection);
        state.update(cx, |state, _| {
            state.delegate_mut()._native_selection = Some(subscription)
        });
        state
    }

    fn on_native_selection(state: Entity<TableState<Self>>, event: &TableEvent, cx: &mut App) {
        if matches!(
            event,
            TableEvent::SelectRow(_) | TableEvent::SelectColumn(_) | TableEvent::SelectCell(_, _)
        ) {
            state.update(cx, Self::clear_native_selection);
        }
    }

    fn clear_native_selection(state: &mut TableState<Self>, cx: &mut Context<TableState<Self>>) {
        // Native key actions can set a selection even when selection flags are
        // disabled; the browser's id set is the only trash selection model.
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

    /// Synchronize the model projection with the latest backend listing.
    pub(in crate::pages::workspace) fn sync_state(
        state: &Entity<TableState<Self>>,
        data: Data,
        cx: &mut App,
    ) {
        let Data {
            rows,
            selected,
            content_width,
        } = data;
        let columns = Self::columns_for(content_width.max(0.), cx);
        state.update(cx, |state, cx| {
            Self::clear_native_selection(state, cx);
            let delegate = state.delegate_mut();
            let refresh = delegate.columns.len() != columns.len()
                || columns
                    .iter()
                    .zip(delegate.columns.iter())
                    .any(|(next, current)| {
                        next.key != current.key
                            || next.name != current.name
                            || next.width != current.width
                    });
            let changed = !Arc::ptr_eq(&delegate.rows, &rows) || delegate.selected != selected;
            delegate.rows = rows;
            delegate.selected = selected;
            delegate.columns = columns;
            if refresh {
                state.refresh(cx);
            }
            if changed || refresh {
                cx.notify();
            }
        });
    }

    fn columns_for(content_width: f32, cx: &App) -> Vec<Column> {
        let name_width = (content_width * NAME_FRACTION).clamp(NAME_MIN_WIDTH, NAME_MAX_WIDTH);
        let columns = [
            ("name", i18n::text("name", cx), name_width, NAME_MIN_WIDTH),
            (
                "original-location",
                i18n::text("recycle-original-location", cx),
                ORIGINAL_WIDTH,
                ORIGINAL_MIN_WIDTH,
            ),
            (
                "deleted-at",
                i18n::text("recycle-deleted-at", cx),
                DELETED_WIDTH,
                DELETED_MIN_WIDTH,
            ),
            ("size", i18n::text("size", cx), SIZE_WIDTH, SIZE_MIN_WIDTH),
        ];
        columns
            .into_iter()
            .map(|(key, name, width, min_width)| {
                Column::new(key, name)
                    .width(px(width))
                    .min_width(px(min_width))
                    .paddings(Edges {
                        top: px(DataListStyle::CELL_PADDING_Y),
                        bottom: px(DataListStyle::CELL_PADDING_Y),
                        left: px(DataListStyle::CELL_PADDING_X),
                        right: px(DataListStyle::CELL_PADDING_X),
                    })
                    .resizable(false)
                    .movable(false)
            })
            .collect()
    }

    fn icon(kind: EntryKind) -> IconName {
        match kind {
            EntryKind::Directory => IconName::Folder,
            EntryKind::SymbolicLink => IconName::Link,
            EntryKind::File | EntryKind::Other | EntryKind::Unknown => IconName::File,
        }
    }

    fn row_click_handler(
        &self,
        row_ix: usize,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let browser = self.browser.clone();
        move |event, window, cx| {
            if let Some(browser) = browser.upgrade() {
                browser.update(cx, |browser, cx| {
                    browser.select_trash(row_ix, event.modifiers(), cx);
                    browser.window.focus.focus(window, cx);
                });
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
        let selected = self.selected.contains(&entry.id);
        let foreground = if selected {
            cx.theme().accent_foreground
        } else {
            cx.theme().foreground
        };
        let icon_color = if selected {
            foreground
        } else if entry.kind == EntryKind::Directory {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        let name: SharedString = entry.name.to_string_lossy().into_owned().into();
        let tooltip_name = name.clone();
        let tooltip = move |window: &mut Window, cx: &mut App| {
            Tooltip::new(tooltip_name.clone()).build(window, cx)
        };
        let hint_enabled = self
            .menu_focus
            .as_ref()
            .is_none_or(|focus| !focus.contains_focused(window, cx));
        let icon = Self::icon(entry.kind);
        el! {
            .div {
                # $("trash-name", row_ix); flex; items_center; size_full; min_w_0; overflow_hidden;
                text_size: $(cx.gds().appearance().typography().scaled(DataListStyle::NAME_SIZE));
                text_color: foreground;
                @if (hint_enabled) tooltip_show_delay: TOOLTIP_DELAY; tooltip: tooltip; @end;
            } [
                .div { flex; items_center; min_w_0; gap: NAME_ICON_GAP.px } [
                    icon.Icon::new { size: NAME_ICON_WIDTH.px; text_color: icon_color },
                    .div { min_w_0; truncate } [name],
                ],
            ]
        }
        .into_any_element()
    }

    fn render_meta(
        &self,
        entry: &Entry,
        text: impl IntoElement,
        right_aligned: bool,
        cx: &Context<TableState<Self>>,
    ) -> AnyElement {
        let foreground = if self.selected.contains(&entry.id) {
            cx.theme().accent_foreground
        } else {
            cx.theme().muted_foreground
        };
        el! {
            .div { size_full; flex; items_center; min_w_0; overflow_hidden } [
                .div {
                    w_full; min_w_0; truncate;
                    text_size: $(cx.gds().appearance().typography().scaled(DataListStyle::META_SIZE));
                    text_color: foreground;
                    @if (right_aligned) text_right; @end;
                } [text],
            ]
        }
        .into_any_element()
    }

    fn render_original_location(
        &self,
        row_ix: usize,
        entry: &Entry,
        cx: &Context<TableState<Self>>,
    ) -> AnyElement {
        let text: SharedString = entry
            .original_path
            .as_os_str()
            .to_string_lossy()
            .escape_debug()
            .to_string()
            .into();
        let tooltip_text = text.clone();
        let tooltip = move |window: &mut Window, cx: &mut App| {
            Tooltip::new(tooltip_text.clone()).build(window, cx)
        };
        let foreground = if self.selected.contains(&entry.id) {
            cx.theme().accent_foreground
        } else {
            cx.theme().muted_foreground
        };
        el! {
            .div {
                # $("trash-original-location", row_ix); size_full; flex; items_center; min_w_0; overflow_hidden;
                tooltip_show_delay: TOOLTIP_DELAY; tooltip: tooltip;
            } [
                .div {
                    w_full; min_w_0; truncate;
                    text_size: $(cx.gds().appearance().typography().scaled(DataListStyle::META_SIZE));
                    text_color: foreground;
                } [text],
            ]
        }
        .into_any_element()
    }
}

impl TableDelegate for TrashTable {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let name = self.columns[col_ix].name.clone();
        el! {
            .div {
                # $("trash-column", col_ix);
                size_full; flex; items_center;
                text_size: $(cx.gds().appearance().typography().scaled(DataListStyle::HEADER_SIZE));
                font_weight: FontWeight::NORMAL; text_color: $(cx.theme().muted_foreground);
                when: $(col_ix == 0), m! { pl: $(NAME_ICON_WIDTH + NAME_ICON_GAP).px };
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
        let row = el! { .div { # $("trash-row", row_ix); } };
        let Some(entry) = self.rows.get(row_ix) else {
            return row;
        };
        let selected = self.selected.contains(&entry.id);
        let on_click = self.row_click_handler(row_ix);
        el! {
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
        }
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
        match col_ix {
            0 => self.render_name(row_ix, entry, window, cx),
            1 => self.render_original_location(row_ix, entry, cx),
            2 => self.render_meta(
                entry,
                Timestamp::new(("trash-deleted", row_ix), entry.deleted_at),
                false,
                cx,
            ),
            3 => {
                let text = size_text(entry, cx);
                self.render_meta(entry, text, true, cx)
            }
            _ => el! { .div }.into_any_element(),
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
        // The popup owns its captured target; clear Kit's visual target after
        // this borrow ends.
        cx.defer_in(window, Self::clear_menu_target);
        if self.rows.get(row_ix).is_none() {
            return menu;
        }
        let Some(browser) = self.browser.upgrade() else {
            return menu;
        };
        // Omitting the name tooltip builder drops both visible and waiting hints.
        self.menu_focus = Some(menu.focus_handle(cx));
        cx.notify();
        browser.update(cx, |browser, cx| {
            browser.trash_row_context_menu(row_ix, menu, window, cx)
        })
    }
}

fn size_text(entry: &Entry, cx: &App) -> SharedString {
    if let Some(items) = entry.items {
        return i18n::count("item-count", items, cx);
    }
    match entry.size {
        Some(size) => EntryPresentation::format_size(size).into(),
        None => UNKNOWN_VALUE.into(),
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn trash_row_context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PopupMenu {
        if self.collection() != Some(Collection::Trash) {
            return menu;
        }
        self.select_trash_row_for_menu(row_ix);
        self.window.focus.focus(window, cx);
        super::view::entry_menu(self, menu, cx)
    }
}
