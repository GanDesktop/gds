use super::*;
use crate::{
    appearance::tokens::{self, DataListStyle},
    i18n::t,
    ui::{IconButton, ListSurface, button::Button, field, picker::*},
};
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{
        breadcrumb::{Breadcrumb, BreadcrumbItem},
        empty::{Empty, EmptyDescription, EmptyHeader, EmptyTitle},
        spinner::Spinner,
        table::DataTable,
        *,
    },
    div,
    prelude::*,
};

/// Maximum height of the picker body inside the modal frame.
const MAX_PICKER_HEIGHT: f32 = 440.;
/// Minimum height of the picker body so short windows stay usable.
const MIN_PICKER_HEIGHT: f32 = 200.;
/// Extra rows kept beyond the tallest viewport so scroll-driven `DataTable`
/// prefetch can trigger when a directory only partially fills the viewport.
const SCROLL_PREFETCH_MARGIN: usize = 9;
/// Minimum visible rows fetched eagerly before stopping page prefetch.
///
/// Exceeds `ceil(MAX_PICKER_HEIGHT / DataListStyle::ROW_HEIGHT)` (`11`) plus
/// `SCROLL_PREFETCH_MARGIN` (`9`) so filtered or hidden-heavy views overflow the
/// viewport enough for scroll-driven `TableDelegate::load_more` without draining
/// every subsequent page.
pub(super) const MIN_SCROLLABLE_ROWS: usize = (MAX_PICKER_HEIGHT as usize)
    .div_ceil(DataListStyle::ROW_HEIGHT as usize)
    + SCROLL_PREFETCH_MARGIN;
/// Remaining-row scroll-prefetch threshold passed to `TableDelegate`.
///
/// Kept strictly smaller than `SCROLL_PREFETCH_MARGIN` so initial renders with
/// `MIN_SCROLLABLE_ROWS` at scroll top do not immediately trigger `load_more`.
pub(super) const TABLE_LOAD_MORE_THRESHOLD: usize = 5;

impl DirectoryPicker {
    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        el! { .PickerToolbar::default [
            "picker-back".IconButton::new { icon: IconName::ArrowLeft; label: t!(@cx, "picker-back"); shortcut: $(commands::shortcut(commands::Back)); disabled: $(self.history.back.is_empty()); on_click: $(cx.listener(|this, _, window, cx| this.back(window, cx))); },
            "picker-forward".IconButton::new { icon: IconName::ArrowRight; label: t!(@cx, "picker-forward"); shortcut: $(commands::shortcut(commands::Forward)); disabled: $(self.history.forward.is_empty()); on_click: $(cx.listener(|this, _, window, cx| this.forward(window, cx))); },
            "picker-parent".IconButton::new { icon: IconName::ArrowUp; label: t!(@cx, "picker-parent"); shortcut: $(commands::shortcut(commands::Parent)); disabled: $(self.page.as_ref().and_then(|page| page.parent.as_ref()).is_none()); on_click: $(cx.listener(|this, _, window, cx| this.parent(window, cx))); },
            .PickerLocation::default [$(self.location(cx))],
            "picker-location-edit".IconButton::new { icon: IconName::Pencil; label: t!(@cx, "picker-location"); shortcut: $(commands::shortcut(commands::EditLocation)); on_click: $(cx.listener(|this, _, window, cx| this.edit_location(window, cx))); },
            @if (self.pending.is_some())
                "picker-stop".IconButton::new { icon: IconName::X; label: t!(@cx, "picker-stop"); on_click: $(cx.listener(|this, _, window, cx| this.cancel_loading(window, cx))); },
            @else
                "picker-refresh".IconButton::new { icon: IconName::RefreshCw; label: t!(@cx, "picker-refresh"); shortcut: $(commands::shortcut(commands::Refresh)); on_click: $(cx.listener(|this, _, window, cx| this.refresh(window, cx))); },
            @end,
        ] }
    }

    fn location(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.editing || self.page.is_none() {
            return el! { ($&self.address, cx).field::input { small; w_full; } }.into_any_element();
        }
        let page = self.page.as_ref().expect("loaded location");
        let ancestors = page.ancestors.iter().map(|location| {
            let path = location.path.clone();
            let label = location.label.clone();
            el! { label.BreadcrumbItem::new {
                on_click: $(cx.listener(move |this, _, window, cx| this.navigate(Some(path.clone()), Navigation::Visit, window, cx)));
            } }
        }).collect::<Vec<_>>();
        let current = el! { ($(page.location.label.clone())).BreadcrumbItem::new { max_w: 180.0.px; truncate; } };
        let full_path = page.location.path.clone();
        el! { .div { # "picker-location"; w_full; min_w_0; overflow_x_scroll;
            tooltip: $move |window, cx| crate::ui::tooltip::view(full_path.clone(), window, cx);
        } [
            .Breadcrumb::new { children: ancestors; child: current; text_size: tokens::SMALL; },
        ] }
        .into_any_element()
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some((request, kind)) = &self.pending
            && *kind != Navigation::More
        {
            return el! { .Empty::default { flex_1; } [
                .Spinner::new,
                .EmptyHeader::default {
                    title: .EmptyTitle::default [t!(@cx, "picker-loading")];
                    description: .EmptyDescription::default [$(request.path.clone().unwrap_or_default())];
                },
            ] }.into_any_element();
        }
        if let Some((_, _, error)) = &self.failed {
            return el! { .Empty::default { flex_1; } [
                .EmptyHeader::default {
                    title: .EmptyTitle::default [t!(@cx, "picker-failed")];
                    description: .EmptyDescription::default [$(crate::ui::tooltip::wrap_tokens(error))];
                },
                "picker-retry".Button::new { label: t!(@cx, "picker-retry"); icon: IconName::RefreshCw; on_click: $(cx.listener(|this, _, window, cx| this.refresh(window, cx))); },
            ] }.into_any_element();
        }
        let table = self.table.read(cx);
        let rows = table.delegate().rows.len();
        let scroll = table.vertical_scroll_handle.0.borrow().base_handle.clone();
        let content = el! { ($&self.table).DataTable::new {
            bordered: false; stripe: false; scrollbar_visible: true, false;
            header_row_height: DataListStyle::HEADER_HEIGHT.px;
            with_size: $(gpui_kit::component::Size::Size(px(DataListStyle::ROW_HEIGHT)));
            accessibility_label: t!(@cx, "picker-folders");
        } }
        .into_any_element();
        let table = self.table.downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                let width = (bounds.size.width - px(DataListStyle::SCROLLBAR_LANE)).max(px(80.));
                if let Some(table) = table.upgrade()
                    && table.read(cx).delegate().width != width
                {
                    cx.defer(move |cx| {
                        table.update(cx, |table, cx| {
                            table.delegate_mut().width = width;
                            table.refresh(cx);
                            cx.notify();
                        })
                    });
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
        el! { .div { relative; flex; flex_col; flex_1; min_h_0; min_w_0; } [
            ("picker-list", content, rows, scroll).ListSurface::new {
                on_blank_press: $(cx.listener(|this, _, _, cx| {
                    this.table.update(cx, |table, cx| table.clear_selection(cx)); cx.notify();
                }));
            },
            measure,
        ] }
        .into_any_element()
    }
}

impl Render for DirectoryPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = (window.bounds().size.height - px(190.))
            .min(px(MAX_PICKER_HEIGHT))
            .max(px(MIN_PICKER_HEIGHT));
        let chosen = self
            .selected_entry(cx)
            .map(|entry| entry.location.path.clone())
            .or_else(|| self.page.as_ref().map(|page| page.location.path.clone()))
            .unwrap_or_default();
        el! { .Picker::default { # "directory-picker"; key_context: "DirectoryPicker"; h: height;
            on_action: $(cx.listener(|this, _: &commands::Back, window, cx| this.back(window, cx)));
            on_action: $(cx.listener(|this, _: &commands::Forward, window, cx| this.forward(window, cx)));
            on_action: $(cx.listener(|this, _: &commands::Parent, window, cx| this.parent(window, cx)));
            on_action: $(cx.listener(|this, _: &commands::Refresh, window, cx| this.refresh(window, cx)));
            on_action: $(cx.listener(|this, _: &commands::EditLocation, window, cx| this.edit_location(window, cx)));
            on_action: $(cx.listener(|this, _: &commands::Hidden, window, cx| this.toggle_hidden(window, cx)));
            on_action: $(cx.listener(|this, _: &commands::Filter, window, cx| this.filter.update(cx, |input, cx| input.focus(window, cx))));
            on_action: $(cx.listener(|this, _: &commands::Open, window, cx| this.open_selected(window, cx)));
        } [
            $(self.toolbar(cx)),
            .PickerToolbar::default [
                .div { flex_1; min_w_0; } [($&self.filter, cx).field::input { small; cleanable: true; w_full; prefix: IconName::Search.Icon::new; }],
                "picker-hidden".IconButton::new { icon: IconName::Eye; label: t!(@cx, "picker-hidden"); toggled: $(self.hidden); shortcut: $(commands::shortcut(commands::Hidden)); on_click: $(cx.listener(|this, _, window, cx| this.toggle_hidden(window, cx))); },
            ],
            .PickerContent::default [$(self.body(cx))],
            .PickerStatus::default { text_color: $(cx.theme().muted_foreground); } [
                @if (self.pending.is_some()) .Spinner::new { small; }, @end,
                .div { # "picker-chosen-path"; min_w_0; flex_1; truncate; text_size: DataListStyle::META_SIZE.px;
                    tooltip: $(move |window, cx| crate::ui::tooltip::view(chosen.clone(), window, cx));
                } [$(self.selected_entry(cx).map_or_else(|| self.page.as_ref().map(|page| page.location.path.clone()).unwrap_or_default(), |entry| entry.location.path.clone()))],
            ],
        ] }
    }
}
