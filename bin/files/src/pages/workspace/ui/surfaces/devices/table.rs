//! The volume overview uses the same table and density as file lists. Each
//! volume owns one row; its browsing root is an editable property of that row.

use super::*;
use gds::appearance::tokens::{self, DataListStyle};
use gds::ui::button::Button;
use gds_uim::el;
use gpui_kit::component::{
    Sizable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenu},
    table::{Column, TableDelegate, TableEvent, TableSelection, TableState},
};

const FORMAT_WIDTH: f32 = 76.;
const MIN_WIDTH: f32 = 700.;
const MIN_NAME_WIDTH: f32 = 172.;
const MIN_SPACE_WIDTH: f32 = 220.;
const MIN_MOUNT_WIDTH: f32 = 170.;
const NAME_FRACTION: f32 = 0.22;
const SPACE_FRACTION: f32 = 0.36;
const CELL_GAP: f32 = 8.;

#[derive(Clone, PartialEq)]
pub(in crate::pages::workspace) struct Row {
    pub volume: storage::Volume,
    pub root: Option<PathBuf>,
    pub capacity: Option<storage::CapacityState>,
    pub icon: IconName,
}

pub(in crate::pages::workspace) struct VolumeTable {
    browser: WeakEntity<Browser>,
    rows: Vec<Row>,
    selected: Option<VolumeId>,
    columns: Vec<Column>,
    menu_focus: Option<FocusHandle>,
    _native_selection: Option<Subscription>,
}

impl VolumeTable {
    pub(in crate::pages::workspace) fn make_state(
        browser: WeakEntity<Browser>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TableState<Self>> {
        let columns = Self::columns(MIN_WIDTH, cx);
        let state = cx.new(|cx| {
            TableState::new(
                Self {
                    browser,
                    rows: Vec::new(),
                    selected: None,
                    columns,
                    menu_focus: None,
                    _native_selection: None,
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
        // Kit actions can select a row even with selection flags disabled.
        // Volume identities, rather than native row indices, own the selection.
        if !matches!(state.selection(), TableSelection::None) {
            state.clear_selection(cx);
        }
    }

    fn columns(width: f32, cx: &App) -> Vec<Column> {
        let flexible = width.max(MIN_WIDTH) - FORMAT_WIDTH;
        let name = (flexible * NAME_FRACTION).max(MIN_NAME_WIDTH);
        let space = (flexible * SPACE_FRACTION).max(MIN_SPACE_WIDTH);
        let mount = (flexible - name - space).max(MIN_MOUNT_WIDTH);
        [
            ("name", "name", name),
            ("format", "volume-format", FORMAT_WIDTH),
            ("space", "volume-space", space),
            ("mount", "volume-default-mount", mount),
        ]
        .into_iter()
        .map(|(key, label, width)| {
            Column::new(key, i18n::text(label, cx))
                .width(px(width))
                .min_width(px(width))
                .movable(false)
                .resizable(false)
                .paddings(Edges {
                    top: px(0.),
                    bottom: px(0.),
                    left: px(DataListStyle::CELL_PADDING_X),
                    right: px(DataListStyle::CELL_PADDING_X),
                })
        })
        .collect()
    }

    pub(in crate::pages::workspace) fn sync(
        state: &Entity<TableState<Self>>,
        rows: Vec<Row>,
        selected: Option<VolumeId>,
        width: f32,
        cx: &mut App,
    ) {
        let columns = Self::columns(width, cx);
        state.update(cx, |state, cx| {
            Self::clear_native_selection(state, cx);
            let delegate = state.delegate_mut();
            let refresh = delegate
                .columns
                .iter()
                .zip(&columns)
                .any(|(old, new)| old.width != new.width || old.name != new.name);
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

    fn root_selector(&self, row: &Row, cx: &App) -> AnyElement {
        let label: SharedString = row
            .root
            .as_ref()
            .map(|root| root.display().to_string())
            .unwrap_or_else(|| "—".into())
            .into();
        let missing = row
            .root
            .as_ref()
            .is_some_and(|root| !row.volume.mount_points.contains(root));
        let tooltip = if missing {
            format!(
                "{}\n{}",
                label,
                i18n::text("volume-default-unavailable", cx)
            )
            .into()
        } else {
            label.clone()
        };
        if row.volume.mount_points.len() <= 1 {
            return el! {
                .div { # format!("volume-default-{}", row.volume.object); min_w_0; truncate; text_xs;
                    tooltip_show_delay: tokens::TOOLTIP_DELAY;
                    tooltip: $(move |window, cx| ui::tooltip::view(tooltip.clone(), window, cx));
                } [label]
            }
            .into_any_element();
        }
        el! { $(format!("volume-default-{}", row.volume.object)).Button::new {
            ghost;
            xsmall;
            w_full;
            min_w_0;
            justify_start;
            label: label;
            accessibility_label: $(i18n::text("volume-default-mount", cx));
            tooltip: tooltip;
            dropdown_caret: true;
            dropdown_menu: $(Browser::default_mount_menu(
                self.browser.clone(),
                row.volume.id.clone(),
            ));
        } }
        .into_any_element()
    }
}

impl TableDelegate for VolumeTable {
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
        el! { .div { size_full; flex; items_center; text_size: $(cx.gds_ro().appearance().typography().scaled(DataListStyle::HEADER_SIZE)); text_color: $cx.theme().muted_foreground } [$(self.columns[index].name.clone())] }
    }

    fn render_tr(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let row = &self.rows[index];
        let browser = self.browser.clone();
        let id = row.volume.id.clone();
        let selected = self.selected.as_ref() == Some(&id);
        let on_click = move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
            // The context menu already selected its target on mouse-down.
            // Its mouse-up must not take focus back from the popup.
            if event.is_right_click() {
                return;
            }
            let _ = browser.update(cx, |view, cx| {
                view.select_volume(id.clone(), window, cx);
                if event.click_count() == 2 {
                    view.show_volume(id.clone(), window, cx);
                }
            });
        };
        el! { .div {
            # $("volume-row", index); relative; border_b_0;
            rounded: DataListStyle::ROW_RADIUS.px; cursor_pointer; on_click: on_click;
        } [
            @if (selected)
                .div {
                    absolute; inset_0; top: DataListStyle::SELECTION_INSET.px; bottom: DataListStyle::SELECTION_INSET.px;
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
        let row = &self.rows[index];
        let volume = &row.volume;
        let selected = self.selected.as_ref() == Some(&volume.id);
        let foreground = if selected {
            cx.theme().accent_foreground
        } else {
            cx.theme().foreground
        };
        let content = match column {
            0 => {
                let tooltip: SharedString = volume.name.clone().into();
                let hint_enabled = self.menu_focus.as_ref().is_none_or(|focus| !focus.contains_focused(window, cx));
                el! { .div { # $("volume-name", index); flex; items_center; size_full; min_w_0; gap: CELL_GAP.px;
                    @if (hint_enabled)
                        tooltip_show_delay: tokens::TOOLTIP_DELAY;
                        tooltip: $(move |window, cx| ui::tooltip::view(tooltip.clone(), window, cx));
                    @end;
                } [
                    $(row.icon).Icon::new { size: tokens::SIDEBAR_ICON_SIZE.px; flex_shrink_0 },
                    .div { min_w_0; truncate; text_size: $(cx.gds_ro().appearance().typography().scaled(DataListStyle::NAME_SIZE)) } [$(volume.name.clone())],
                    @if (volume.system_mount)
                        .div { flex_shrink_0; px: DataListStyle::CELL_PADDING_Y.px; py: 1.0.px;
                            rounded: DataListStyle::ROW_RADIUS.px; bg: $cx.theme().secondary;
                            text_size: $(cx.gds_ro().appearance().typography().scaled(DataListStyle::HEADER_SIZE)); text_color: $cx.theme().muted_foreground;
                        } [$i18n::text("device-system-volume", cx)],
                    @end,
                ] }.into_any_element()
            }
            1 => el! { .div { text_xs; truncate } [$(if volume.filesystem.is_empty() { "—".to_owned() } else { volume.filesystem.clone() })] }.into_any_element(),
            2 if volume.mount_points.is_empty() => el! { .div { text_xs; text_color: $(if selected { foreground } else { cx.theme().muted_foreground }) } [$i18n::text("device-not-mounted", cx)] }.into_any_element(),
            2 => el! { .DeviceCapacity::default {
                # format!("volume-table-capacity-{}", volume.object); state: $(row.capacity.clone());
                selected: selected; embedded: true;
            } }.into_any_element(),
            _ => self.root_selector(row, cx),
        };
        el! { .div { size_full; min_w_0; flex; items_center; text_color: foreground } [content] }
            .into_any_element()
    }

    fn render_last_empty_col(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        el! { .div { w: 0.0.px; h_full; flex_shrink_0 } }
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
            Self::clear_native_selection(state, cx);
        });
        let Some(row) = self.rows.get(index) else {
            return menu;
        };
        let id = row.volume.id.clone();
        let menu_focus = menu.focus_handle(cx);
        self.menu_focus = Some(menu_focus.clone());
        // Finish the row's pointer handling before the popup takes its keys.
        window.on_next_frame(move |window, cx| menu_focus.focus(window, cx));
        cx.notify();
        let available = self
            .browser
            .update(cx, |view, cx| {
                view.select_volume(id.clone(), window, cx);
                view.volume_can_open(&id, cx)
            })
            .unwrap_or(false);
        if available {
            Browser::prepare_volume_open(&menu, window, cx);
        }
        Browser::volume_menu_contents(self.browser.clone(), &id, menu, cx)
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        el! { .div { size_full; flex; items_center; justify_center; text_sm; text_color: $cx.theme().muted_foreground } [$i18n::text("device-none", cx)] }
    }
}
