use super::*;
use crate::{
    app::AppExt as _,
    appearance::tokens::{self, DataListStyle},
    i18n::t,
};
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Icon,
        table::{Column, TableDelegate},
    },
    prelude::*,
};

pub(super) struct Directories {
    picker: WeakEntity<DirectoryPicker>,
    pub rows: Vec<Entry>,
    pub selected: Option<usize>,
    pub width: Pixels,
}
impl Directories {
    pub fn state(
        picker: WeakEntity<DirectoryPicker>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TableState<Self>> {
        cx.new(|cx| {
            TableState::new(
                Self {
                    picker,
                    rows: Vec::new(),
                    selected: None,
                    width: px(320.),
                },
                window,
                cx,
            )
            .col_selectable(false)
            .cell_selectable(false)
            .sortable(false)
            .col_resizable(false)
            .col_movable(false)
        })
    }
}
impl TableDelegate for Directories {
    fn columns_count(&self, _: &App) -> usize {
        1
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, _: usize, cx: &App) -> Column {
        Column::new("directory", t!(@cx, "picker-name")).width(self.width)
    }
    fn render_th(
        &mut self,
        _: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        el! { .div { text_size: DataListStyle::HEADER_SIZE.px; text_color: $(cx.theme().muted_foreground); } [t!(@cx, "picker-name")] }
    }
    fn render_tr(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let color = if self.selected == Some(index) {
            cx.gds_ro()
                .appearance()
                .theme()
                .colors()
                .selection_foreground
        } else {
            cx.theme().foreground
        };
        el! { .div { # format!("directory-row-{index}"); rounded: DataListStyle::ROW_RADIUS.px; text_color: color; } }
    }
    fn render_td(
        &mut self,
        index: usize,
        _: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let entry = &self.rows[index];
        let path = entry.location.path.clone();
        el! { .div { # format!("directory-name-{index}"); test_support; flex; items_center; gap_2; size_full; min_w_0;
            text_size: DataListStyle::NAME_SIZE.px;
            when: $(entry.hidden && self.selected != Some(index)), { opacity: 0.65; };
            tooltip: $move |window, cx| crate::ui::tooltip::view(path.clone(), window, cx);
        } [
            IconName::Folder.Icon::new { size: tokens::SIDEBAR_ICON_SIZE.px; flex_shrink_0; },
            .div { min_w_0; flex_1; truncate; } [$(entry.location.label.clone())],
        ] }
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        use gpui_kit::component::empty::{Empty, EmptyHeader, EmptyTitle};
        let filtered = self
            .picker
            .upgrade()
            .is_some_and(|picker| !picker.read(cx).filter.read(cx).value().is_empty());
        el! { .Empty::default { size_full; } [
            IconName::Folder.Icon::new { size: 24.0.px; },
            .EmptyHeader::default {
                title: .EmptyTitle::default [$(if filtered { t!(@cx, "picker-no-matches") } else { t!(@cx, "picker-empty") })];
            },
        ] }
    }

    fn cell_label(&self, index: usize, _: usize, _: &App) -> Option<SharedString> {
        Some(self.rows[index].location.label.clone().into())
    }
    fn has_more(&self, cx: &App) -> bool {
        self.picker.upgrade().is_some_and(|picker| {
            let picker = picker.read(cx);
            picker.pending.is_none()
                && picker.failed.is_none()
                && picker
                    .page
                    .as_ref()
                    .is_some_and(|page| page.next_cursor.is_some())
        })
    }
    fn load_more_threshold(&self) -> usize {
        super::view::TABLE_LOAD_MORE_THRESHOLD
    }
    fn load_more(&mut self, window: &mut Window, _cx: &mut Context<TableState<Self>>) {
        let picker = self.picker.clone();
        window.on_next_frame(move |window, cx| {
            let _ = picker.update(cx, |picker, cx| picker.load_more(window, cx));
        });
    }
}
