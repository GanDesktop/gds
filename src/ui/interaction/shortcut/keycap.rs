use super::*;
use crate::app::AppExt as _;
use gds_uim::el;
use gpui_kit::IntoElement;
use gpui_kit::component::ActiveTheme as _;

const KEY_HEIGHT: f32 = 22.;
const KEY_PADDING: f32 = 6.;
const KEY_TEXT_SIZE: f32 = 12.;

#[derive(IntoElement)]
pub(super) struct Keycap {
    label: SharedString,
}

impl Keycap {
    pub(super) fn new(label: SharedString) -> Self {
        Self { label }
    }
}

impl RenderOnce for Keycap {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let label = self.label;
        el! {
            $(gpui_kit::component::kbd::Kbd::new(Keystroke { key: label.to_string(), ..Keystroke::default() })) {
                flex; items_center; justify_center;
                h: KEY_HEIGHT.px; min_w: KEY_HEIGHT.px; px: KEY_PADDING.px;
                rounded: KEY_GAP.px; border_1;
                border_color: $cx.theme().border;
                bg: $cx.theme().secondary; text_color: $cx.theme().muted_foreground;
                text_size: $(cx.gds().appearance().typography().scaled(KEY_TEXT_SIZE));
            }
        }
    }
}
