//! Theme-aware raised card and translucent glass panel surfaces.

use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::{component, view};

use crate::app::Gds;
use crate::appearance::theme::color::color_css;

const FACE_OPACITY: f32 = 0.7;

/// Bordered raised card container styled by `--gds-panel`.
#[component]
pub fn CardSurface(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    crate::ui::styles::install_ui_stylesheets();
    view! {
        box(class = "gds-card-surface", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}

/// Translucent `20px`-rounded glass panel (`panel` at `opacity * 0.7`, `1px` border, ambient +
/// contact drop shadows, and `1px` inset top rim highlight).
#[component]
pub fn GlassPanel(
    #[prop(into, default = Signal::stored_local(1.0))] opacity: Signal<f32, LocalStorage>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    crate::ui::styles::install_ui_stylesheets();
    let glass_bg = move || {
        let alpha_factor = opacity.get().clamp(0.0, 1.0) * FACE_OPACITY;
        if let Some(gds) = Gds::try_current() {
            let panel = gds.appearance().theme().colors().panel;
            Some(color_css(panel.with_alpha(alpha_factor)))
        } else {
            Some(format!(
                "color-mix(in srgb, var(--gds-panel) {:.1}%, transparent)",
                alpha_factor * 100.0
            ))
        }
    };
    view! {
        box(
            class = "gds-glass-panel",
            var:--gds-glass-bg = glass_bg,
            class = class,
            {..attrs}
        ) {
            {children.into_view_once()}
        }
    }
}
