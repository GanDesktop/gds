use super::*;
use gds::{app::AppExt as _, appearance::tokens::DataListStyle, ui::time::Timestamp};
use gds_uim::{el, m};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Icon,
        table::{Column, TableDelegate},
    },
    prelude::FluentBuilder,
};

const NAME_ICON_WIDTH: f32 = 18.0;
const NAME_ICON_GAP: f32 = 10.0;
const UNKNOWN_VALUE: &str = "—";

struct ColumnSpec {
    key: &'static str,
    width: f32,
    min_width: f32,
}

const COLUMNS: [ColumnSpec; 3] = [
    ColumnSpec {
        key: "remote-name",
        width: 420.0,
        min_width: 200.0,
    },
    ColumnSpec {
        key: "remote-size",
        width: 140.0,
        min_width: 90.0,
    },
    ColumnSpec {
        key: "remote-modified",
        width: 220.0,
        min_width: 140.0,
    },
];

pub(super) struct Files {
    view: WeakEntity<View>,
    pub rows: Arc<[Entry]>,
    pub selected: Selection<Address>,
    pub selection_dirty: bool,
    columns: Vec<Column>,
}

impl Files {
    pub(super) fn state(
        view: WeakEntity<View>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TableState<Self>> {
        let columns = COLUMNS
            .into_iter()
            .map(|spec| {
                Column::new(spec.key, text(spec.key, cx))
                    .width(px(spec.width))
                    .min_width(px(spec.min_width))
                    .paddings(Edges {
                        top: px(DataListStyle::CELL_PADDING_Y),
                        bottom: px(DataListStyle::CELL_PADDING_Y),
                        left: px(DataListStyle::CELL_PADDING_X),
                        right: px(DataListStyle::CELL_PADDING_X),
                    })
                    .resizable(false)
                    .movable(false)
            })
            .collect();
        cx.new(|cx| {
            TableState::new(
                Self {
                    view,
                    rows: Arc::from([]),
                    selected: Selection::new(),
                    selection_dirty: false,
                    columns,
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
        })
    }

    pub(super) fn release_selection(state: &Entity<TableState<Self>>, cx: &mut App) {
        if state.read(cx).delegate().selected.is_empty() {
            return;
        }
        state.update(cx, |state, _| {
            let delegate = state.delegate_mut();
            delegate.selected.clear();
            delegate.selection_dirty = true;
        });
    }

    pub(super) fn format_size(bytes: u64) -> String {
        const KIB: f64 = 1024.0;
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

    fn icon(kind: EntryKind) -> IconName {
        match kind {
            EntryKind::Directory => IconName::Folder,
            EntryKind::SymbolicLink => IconName::Link,
            EntryKind::File | EntryKind::Other => IconName::File,
        }
    }

    fn row_click_handler(
        &self,
        index: usize,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let view = self.view.clone();
        move |event, window, cx| {
            let _ = view.update(cx, |view, cx| view.select(index, event, window, cx));
        }
    }

    fn render_name(
        &self,
        index: usize,
        entry: &Entry,
        selected: bool,
        cx: &Context<TableState<Self>>,
    ) -> AnyElement {
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
        let icon = Self::icon(entry.kind);
        let hidden = entry.name.starts_with('.');
        let name: SharedString = entry.name.clone().into();
        el! {
            .div {
                # $("remote-name", index);
                flex; items_center; size_full; min_w_0; overflow_hidden;
                text_size: $(cx.gds().appearance().typography().scaled(DataListStyle::NAME_SIZE));
                text_color: foreground;
                when: $(hidden && !selected), { opacity: 0.65; };
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
        selected: bool,
        content: impl IntoElement,
        right_aligned: bool,
        cx: &Context<TableState<Self>>,
    ) -> AnyElement {
        let foreground = if selected {
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
                } [content],
            ]
        }
        .into_any_element()
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
        let name = self.columns[index].name.clone();
        el! {
            .div {
                # $("remote-column", index);
                flex; items_center; size_full;
                text_size: $(cx.gds().appearance().typography().scaled(DataListStyle::HEADER_SIZE));
                font_weight: FontWeight::NORMAL;
                text_color: $(cx.theme().muted_foreground);
                when: $(index == 0), m! { pl: $(NAME_ICON_WIDTH + NAME_ICON_GAP).px };
                when: $(index == 1), m! { justify_end };
            } [name]
        }
    }

    fn render_tr(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let row = el! { .div { # $("remote-row", index); } };
        let Some(entry) = self.rows.get(index) else {
            return row;
        };
        let selected = self.selected.contains(&entry.address);
        let on_click = self.row_click_handler(index);
        el! {
            row {
                relative; border_b_0; rounded: DataListStyle::ROW_RADIUS.px;
                cursor_pointer;
                on_click: on_click;
            } [
                @if (selected)
                    .div {
                        absolute; inset_0;
                        top: DataListStyle::SELECTION_INSET.px;
                        bottom: DataListStyle::SELECTION_INSET.px;
                        rounded: DataListStyle::ROW_RADIUS.px;
                        bg: $(cx.theme().selection);
                    },
                @end,
            ]
        }
    }

    fn render_td(
        &mut self,
        index: usize,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(entry) = self.rows.get(index) else {
            return el! { .div }.into_any_element();
        };
        let selected = self.selected.contains(&entry.address);
        match column {
            0 => self.render_name(index, entry, selected, cx),
            1 => {
                let size_text: SharedString = if entry.kind == EntryKind::Directory {
                    UNKNOWN_VALUE.into()
                } else {
                    entry.size.map_or_else(
                        || UNKNOWN_VALUE.into(),
                        |bytes| Self::format_size(bytes).into(),
                    )
                };
                self.render_meta(selected, size_text, true, cx)
            }
            2 => self.render_meta(
                selected,
                Timestamp::new(("remote-modified", index), entry.modified),
                false,
                cx,
            ),
            _ => el! { .div }.into_any_element(),
        }
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let filtered = self
            .view
            .upgrade()
            .is_some_and(|view| view.read(cx).is_filtered());
        let label = if filtered {
            t!(@cx, "empty-filter")
        } else {
            t!(@cx, "empty-directory")
        };
        el! {
            .div {
                size_full; flex; items_center; justify_center;
                text_size: $(cx.gds().appearance().typography().scaled(DataListStyle::NAME_SIZE));
                text_color: $(cx.theme().muted_foreground);
            } [label]
        }
    }

    fn render_last_empty_col(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        el! { .div { w: 0.0.px; h_full; flex_shrink_0; } }
    }
}
