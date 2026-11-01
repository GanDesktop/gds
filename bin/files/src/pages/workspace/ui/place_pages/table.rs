//! Shared table for recent items, bookmarked folders and mounted network locations.

use super::super::*;
use gds::appearance::tokens;
use gds::appearance::{tokens::DataListStyle, typography};
use gds_uim::el;
use gpui_kit::component::{
    ActiveTheme, Icon,
    menu::PopupMenu,
    table::{Column, TableDelegate, TableEvent, TableSelection, TableState},
    tooltip::Tooltip,
};
use gpui_kit::prelude::*;

const NAME_FRACTION: f32 = 0.35;
const NAME_MIN_WIDTH: f32 = 180.;
const PATH_MIN_WIDTH: f32 = 280.;
const ICON_GAP: f32 = 10.;

pub(in super::super) struct LocationTable {
    browser: WeakEntity<Browser>,
    collection: Collection,
    rows: Vec<Location>,
    selected: Option<Location>,
    columns: Vec<Column>,
    menu_focus: Option<FocusHandle>,
    _selection: Option<Subscription>,
}

impl LocationTable {
    pub(in super::super) fn make_state(
        browser: WeakEntity<Browser>,
        collection: Collection,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TableState<Self>> {
        let columns = Self::columns(0., cx);
        let state = cx.new(|cx| {
            TableState::new(
                Self {
                    browser,
                    collection,
                    rows: Vec::new(),
                    selected: None,
                    columns,
                    menu_focus: None,
                    _selection: None,
                },
                window,
                cx,
            )
            .row_selectable(false)
            .col_selectable(false)
            .cell_selectable(false)
            .col_movable(false)
            .col_resizable(false)
            .sortable(false)
        });
        let subscription = cx.subscribe(&state, Self::on_selection);
        state.update(cx, |state, _| {
            state.delegate_mut()._selection = Some(subscription)
        });
        state
    }

    fn on_selection(state: Entity<TableState<Self>>, event: &TableEvent, cx: &mut App) {
        if matches!(
            event,
            TableEvent::SelectRow(_) | TableEvent::SelectCell(_, _) | TableEvent::SelectColumn(_)
        ) {
            state.update(cx, Self::clear_selection);
        }
    }

    fn clear_selection(state: &mut TableState<Self>, cx: &mut Context<TableState<Self>>) {
        if !matches!(state.selection(), TableSelection::None) {
            state.clear_selection(cx);
        }
    }

    fn columns(width: f32, cx: &App) -> Vec<Column> {
        let name = (width * NAME_FRACTION).max(NAME_MIN_WIDTH);
        [
            ("name", "name", name),
            (
                "location",
                "place-location",
                (width - name).max(PATH_MIN_WIDTH),
            ),
        ]
        .into_iter()
        .map(|(key, label, width)| {
            Column::new(key, i18n::text(label, cx))
                .width(px(width))
                .min_width(px(width))
                .resizable(false)
                .movable(false)
                .paddings(Edges {
                    top: px(DataListStyle::CELL_PADDING_Y),
                    bottom: px(DataListStyle::CELL_PADDING_Y),
                    left: px(DataListStyle::CELL_PADDING_X),
                    right: px(DataListStyle::CELL_PADDING_X),
                })
        })
        .collect()
    }

    pub(in super::super) fn sync(
        state: &Entity<TableState<Self>>,
        rows: Vec<Location>,
        selected: Option<Location>,
        width: f32,
        cx: &mut App,
    ) {
        let columns = Self::columns(width, cx);
        state.update(cx, |state, cx| {
            Self::clear_selection(state, cx);
            let delegate = state.delegate_mut();
            let refresh = delegate
                .columns
                .iter()
                .zip(&columns)
                .any(|(old, new)| old.name != new.name || old.width != new.width);
            let changed = delegate.rows != rows || delegate.selected != selected;
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
}

impl TableDelegate for LocationTable {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, index: usize, _: &App) -> Column {
        self.columns[index].clone()
    }

    fn render_th(
        &mut self,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        el! { .div { size_full; flex; items_center;
            text_size: $(typography::scaled(DataListStyle::HEADER_SIZE, cx)); text_color: $cx.theme().muted_foreground;
        } [$(self.columns[column].name.clone())] }
    }

    fn render_tr(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let location = self.rows[index].clone();
        let selected = self.selected.as_ref() == Some(&location);
        let collection = self.collection;
        let browser = self.browser.clone();
        let on_click = move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
            if event.is_right_click() {
                return;
            }
            let _ = browser.update(cx, |browser, cx| {
                browser.select_place(collection, location.clone(), window, cx);
                if event.click_count() == 2 {
                    browser.open_place_cursor(cx);
                }
            });
        };
        el! { .div { # $("location-row", index); relative; border_b_0; cursor_pointer;
            rounded: DataListStyle::ROW_RADIUS.px; on_click: on_click;
        } [
            @if (selected)
                .div { absolute; inset_0; top: DataListStyle::SELECTION_INSET.px; bottom: DataListStyle::SELECTION_INSET.px;
                    rounded: DataListStyle::ROW_RADIUS.px; bg: $cx.theme().selection;
                },
            @end,
        ] }
    }

    fn render_td(
        &mut self,
        index: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let location = &self.rows[index];
        let selected = self.selected.as_ref() == Some(location);
        let foreground = if selected {
            cx.theme().accent_foreground
        } else {
            cx.theme().foreground
        };
        let name = location.label(cx);
        let icon = location.icon();
        let address = location.display_address();
        let tooltip: SharedString = address.clone().into();
        let hint_enabled = self
            .menu_focus
            .as_ref()
            .is_none_or(|focus| !focus.contains_focused(window, cx));
        el! { .div { size_full; flex; items_center; min_w_0; gap: ICON_GAP.px; text_color: foreground;
            # $("location-cell", index * 2 + column);
            @if (hint_enabled)
                tooltip_show_delay: tokens::TOOLTIP_DELAY;
                tooltip: $(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx));
            @end;
            text_size: $(typography::scaled(if column == 0 { DataListStyle::NAME_SIZE } else { DataListStyle::META_SIZE }, cx));
        } [
            @if (column == 0) icon.Icon::new { size: tokens::SIDEBAR_ICON_SIZE.px; flex_shrink_0 }, @end,
            .div { min_w_0; truncate } [$(if column == 0 { name } else { address })],
        ] }
    }

    fn context_menu(
        &mut self,
        index: usize,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        cx.defer_in(window, |state, _, cx| {
            state.set_right_clicked_row(None, cx);
            Self::clear_selection(state, cx);
        });
        let Some(location) = self.rows.get(index).cloned() else {
            return menu;
        };
        let collection = self.collection;
        self.menu_focus = Some(menu.focus_handle(cx));
        cx.notify();
        let Some(browser) = self.browser.upgrade() else {
            return menu;
        };
        browser.update(cx, |browser, cx| {
            browser.select_place(collection, location, window, cx);
            browser.place_selection_menu(menu, cx)
        })
    }

    fn render_last_empty_col(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        el! { .div { w: 0.0.px; h_full; flex_shrink_0 } }
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let message = if self.collection == Collection::Network {
            if self
                .browser
                .upgrade()
                .is_some_and(|browser| browser.read(cx).network.loading)
            {
                "network-loading"
            } else {
                "network-empty"
            }
        } else {
            "empty-collection"
        };
        el! { .div { size_full; flex; items_center; justify_center; text_color: $cx.theme().muted_foreground } [$i18n::text(message, cx)] }
    }
}
