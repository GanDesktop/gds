//! Common footer controls and removable list rows.
use super::*;
use gds_uim::{component, el, m};
use gpui_kit::IntoElement;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Icon, ThemeStyled, button::ButtonVariants as _, list::ListItem,
        popover::Popover, tooltip::Tooltip,
    },
    prelude::*,
};

const ICON_SIZE: f32 = 14.;
const LABEL_WIDTH: f32 = 240.;
const LABEL_GAP: f32 = 8.;
const CONTROL_SIZE: f32 = 32.;
const BADGE_HEIGHT: f32 = 12.;
const TEXT_SIZE_REMS: f32 = 0.75;
const LIST_WIDTH: f32 = 320.;
const LIST_HEIGHT: f32 = 320.;
const ROW_HEIGHT: f32 = 32.;
const LABEL_LINE_HEIGHT: f32 = 1.5;

#[component]
#[derive(IntoElement)]
pub(super) struct Summary {
    #[attr]
    id: &'static str,
    #[attr]
    compact: bool,
    #[attr]
    icon: IconName,
    #[attr(into)]
    label: SharedString,
    #[attr]
    count: usize,
}
impl Default for Summary {
    fn default() -> Self {
        Self {
            id: "status-summary",
            compact: false,
            icon: IconName::ListTodo,
            label: SharedString::default(),
            count: 0,
        }
    }
}
impl RenderOnce for Summary {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let expanded = motion::state_progress(self.id, !self.compact, window, cx);
        let label_width = window
            .text_system()
            .shape_line(
                self.label.clone(),
                window.rem_size() * TEXT_SIZE_REMS,
                &[window.text_style().to_run(self.label.len())],
                None,
            )
            .width
            .ceil()
            .min(px(LABEL_WIDTH));
        let label_height = (window.rem_size() * TEXT_SIZE_REMS * LABEL_LINE_HEIGHT).ceil();
        el! { .div { flex; items_center; text_xs; line_height: $(relative(LABEL_LINE_HEIGHT)); } [
            .div { # $(format!("{}-icon", self.id)); test_support; relative; flex; items_center; justify_center; flex_shrink_0; size: CONTROL_SIZE.px; } [
                $(Icon::new(self.icon).size(px(ICON_SIZE))),
                @if (self.count > 0 && expanded < 1.)
                    .div { # $(format!("{}-badge", self.id)); test_support; aria_label: $(self.count.to_string());
                        absolute; top: 2.0.px; right: 1.0.px; rounded_full; px_1; min_w: BADGE_HEIGHT.px; h: BADGE_HEIGHT.px;
                        flex; items_center; justify_center; line_height: $(relative(1.)); text_size: 9.0.px;
                        opacity: $(1. - expanded); bg: $(cx.theme().primary); text_color: $(cx.theme().primary_foreground);
                    } [$(self.count.to_string())],
                @end,
            ],
            @if (!self.label.is_empty() && expanded > 0.)
                .div { # $(format!("{}-label", self.id)); test_support; aria_label: $(self.label.clone());
                    h: label_height; w: $(label_width * expanded); mr: $(px(LABEL_GAP * expanded));
                    overflow_hidden; opacity: expanded; flex_shrink_0;
                } [
                    .div { h: label_height; w: label_width; truncate; } [$(self.label)],
                ],
            @end,
        ] }
    }
}

pub(super) fn control<V: Render + 'static>(
    id: &'static str,
    title: SharedString,
    summary: Summary,
    details: Entity<V>,
) -> AnyElement {
    let trigger = ui::button::Button::new(id)
        .ghost()
        .px_0()
        .h(px(CONTROL_SIZE))
        .text_xs()
        .accessibility_label(title.clone())
        .tooltip(title)
        .child(summary.id(id));
    Popover::new(id)
        .appearance(false)
        .anchor(Anchor::BottomLeft)
        .trigger(trigger)
        .content(move |_, _, cx| div().popover_style(cx).child(details.clone()))
        .into_any_element()
}

pub(super) fn list(rows: Vec<AnyElement>, empty: SharedString) -> AnyElement {
    el! { .div { # "status-details"; w: LIST_WIDTH.px; max_w_full; max_h: LIST_HEIGHT.px; overflow_y_scroll; flex; flex_col; p_1; text_sm; } [
        @if (rows.is_empty()) .div [empty], @end,
        ..rows,
    ] }.into_any_element()
}

pub(super) fn row(
    id: SharedString,
    label: super::file_label::FileLabel,
    title: SharedString,
    action: AnyElement,
) -> AnyElement {
    let tooltip =
        move |window: &mut Window, cx: &mut App| Tooltip::new(title.clone()).build(window, cx);
    el! {
        id.ListItem::new { group: "status-row"; h: ROW_HEIGHT.px; w_full; px_2; py_0; text_sm; rounded_md; tooltip: tooltip; } [
            .div { flex; items_center; gap_2; w_full; min_w_0; } [
                .div { flex_1; min_w_0; } [label],
                .div { flex_shrink_0; invisible; group_hover: "status-row", m! { visible; }; } [action],
            ],
        ]
    }.into_any_element()
}
