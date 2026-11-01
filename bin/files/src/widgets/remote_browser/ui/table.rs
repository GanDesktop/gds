use super::*;
use gds::appearance::tokens::{self, DataListStyle};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon,
    table::{Column, TableDelegate},
};

pub(super) struct Files {
    view: WeakEntity<View>,
    pub rows: Vec<Entry>,
    pub selected: Vec<Address>,
    columns: Vec<Column>,
}

impl Files {
    pub(super) fn state(
        view: WeakEntity<View>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TableState<Self>> {
        let columns = [
            ("remote-name", 420.),
            ("remote-size", 140.),
            ("remote-modified", 220.),
        ]
        .into_iter()
        .map(|(name, width)| Column::new(name, i18n::text(name, cx)).width(px(width)))
        .collect();
        cx.new(|cx| {
            TableState::new(
                Self {
                    view,
                    rows: Vec::new(),
                    selected: Vec::new(),
                    columns,
                },
                window,
                cx,
            )
            .row_selectable(false)
            .col_selectable(false)
            .cell_selectable(false)
            .sortable(false)
        })
    }
}

impl TableDelegate for Files {
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
        index: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        el! { .div { flex; items_center; size_full; text_color: $cx.theme().muted_foreground } [$(self.columns[index].name.clone())] }
    }
    fn render_tr(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let view = self.view.clone();
        let selected = self.selected.contains(&self.rows[index].address);
        let on_click = move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| view.select(index, event, window, cx));
        };
        el! { .div { # $("remote-row", index); cursor_pointer; relative; on_click: on_click;
            @if (selected) bg: $cx.theme().selection; @end;
            rounded: DataListStyle::ROW_RADIUS.px;
        } }
    }
    fn render_td(
        &mut self,
        index: usize,
        column: usize,
        _: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let entry = &self.rows[index];
        let content = match column {
            0 => entry.name.clone(),
            1 => entry.size.map_or_else(|| "—".into(), |v| v.to_string()),
            _ => String::new(),
        };
        let content = if column == 2 {
            ui::time::Timestamp::new(("remote-modified", index), entry.modified).into_any_element()
        } else {
            div().min_w_0().truncate().child(content).into_any_element()
        };
        let icon = if entry.kind == EntryKind::Directory {
            IconName::Folder
        } else {
            IconName::File
        };
        el! { .div { flex; items_center; gap_2; min_w_0; size_full; text_size: tokens::SMALL } [
            @if (column == 0) icon.Icon::new { size: tokens::SIDEBAR_ICON_SIZE.px }, @end,
            content,
        ] }
    }
}
