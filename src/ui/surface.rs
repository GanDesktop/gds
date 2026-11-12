//! Theme-aware secondary surfaces for floating controls inside an application.

use crate::app::AppExt as _;
use gpui_kit::{prelude::*, *};

const GLASS_RADIUS: f32 = 20.;
const FACE_OPACITY: f32 = 0.7;
const HIGHLIGHT_OPACITY: f32 = 0.12;
const SHADOW_OPACITY: f32 = 0.28;
const CONTACT_OPACITY: f32 = 0.12;
const SHADOW_OFFSET: f32 = 12.;
const SHADOW_BLUR: f32 = 36.;
const SHADOW_SPREAD: f32 = -10.;
const CONTACT_BLUR: f32 = 8.;
const CONTACT_SPREAD: f32 = -2.;

/// A translucent glass panel with an even tint and a raised rim.
pub fn glass_panel(opacity: f32, cx: &App) -> Div {
    let colors = cx.gds_ro().appearance().theme().colors();
    let highlight = colors.panel_foreground.opacity(HIGHLIGHT_OPACITY);
    let face = colors.panel.opacity(opacity * FACE_OPACITY);
    let shadows = vec![
        BoxShadow::new(
            px(0.),
            px(SHADOW_OFFSET),
            colors.shadow.opacity(SHADOW_OPACITY),
        )
        .blur_radius(px(SHADOW_BLUR))
        .spread_radius(px(SHADOW_SPREAD)),
        BoxShadow::new(px(0.), px(2.), colors.shadow.opacity(CONTACT_OPACITY))
            .blur_radius(px(CONTACT_BLUR))
            .spread_radius(px(CONTACT_SPREAD)),
        BoxShadow::new(px(0.), px(1.), highlight).inset(),
    ];
    div()
        .relative()
        .rounded(px(GLASS_RADIUS))
        .border_1()
        .border_color(colors.border)
        .text_color(colors.panel_foreground)
        .bg(face)
        .shadow(shadows)
}
