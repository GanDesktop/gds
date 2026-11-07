use super::{super::motion, CONTROL_SIZE};
use gds_uim::{component, el};
use gpui_kit::{
    IntoElement,
    assets::IconName,
    component::{ActiveTheme, Icon},
    prelude::*,
    *,
};

const ICON_SIZE: f32 = 14.;
const LABEL_WIDTH: f32 = 240.;
const LABEL_GAP: f32 = 8.;
const BADGE_HEIGHT: f32 = 12.;
const TEXT_SIZE_REMS: f32 = 0.75;
const LABEL_LINE_HEIGHT: f32 = 1.5;

#[component]
#[derive(IntoElement)]
pub(in crate::pages::workspace::ui) struct StatusSummary {
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
impl Default for StatusSummary {
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
impl RenderOnce for StatusSummary {
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
            .div { # format!("{}-icon", self.id); test_support; relative; flex; items_center; justify_center; flex_shrink_0; size: CONTROL_SIZE.px; } [
                $(self.icon).Icon::new { size: ICON_SIZE.px; },
                @if (self.count > 0 && expanded < 1.)
                    .div { # format!("{}-badge", self.id); test_support; aria_label: $(self.count.to_string());
                        absolute; top: 2.0.px; right: 1.0.px; rounded_full; px_1; min_w: BADGE_HEIGHT.px; h: BADGE_HEIGHT.px;
                        flex; items_center; justify_center; line_height: $(relative(1.)); text_size: 9.0.px;
                        opacity: $(1. - expanded); bg: $(cx.theme().primary); text_color: $(cx.theme().primary_foreground);
                    } [$(self.count.to_string())],
                @end,
            ],
            @if (!self.label.is_empty() && expanded > 0.)
                .div { # format!("{}-label", self.id); test_support; aria_label: $(self.label.clone());
                    h: label_height; w: $(label_width * expanded); mr: $(px(LABEL_GAP * expanded));
                    overflow_hidden; opacity: expanded; flex_shrink_0;
                } [
                    .div { h: label_height; w: label_width; truncate; } [$(self.label)],
                ],
            @end,
        ] }
    }
}
