//! Suite settings anatomy and numeric field semantics.
pub mod form;
mod row;
use anti_webui::{
    prelude::*,
    widgets::{SliderOptions, slider_with, switch_with},
};
use gds_uim::el;
pub use row::*;

pub const CONTROL_WIDTH: f32 = 260.0;

pub fn setting_slider(value: Binding<f32>, range: NumberRange, disabled: bool) -> Div {
    slider_with(value, range.min, range.max, SliderOptions::new().step(range.step), move |state| {
        el! { .div.{
            relative; flex; items_center; min_w: 0.0_f32; h: 20.0_f32;
            cursor: $(if disabled { CursorIcon::Default } else {CursorIcon::Pointer});
            focused: s => $(s.outline(1.0_f32).outline_offset(-1.0_f32).outline_color(theme().focus_ring));
        }.[
            .div.{relative; w_full; h: 6.0_f32; rounded: 3.0_f32;
                dyn_style: move s => $(s.bg(if disabled { theme().border.with_alpha(0.7) } else { theme().border }));
            }.[
                .div.{absolute; left: 0.0_f32; top: 0.0_f32; h_full; rounded: 3.0_f32;
                    dyn_style: move s => $(s.w(pct(state.fraction() * 100.0))
                        .bg(theme().accent.with_alpha(if disabled {0.55} else {1.0})));
                },
            ],
            if !disabled [
                .div.{absolute; top: 2.0_f32; size: 16.0_f32; rounded: 8.0_f32; border: 1.0_f32;
                    dyn_style: move s => $(s.left(pct(state.fraction() * 100.0)).translate_x(-16.0 * state.fraction())
                        .bg(theme().accent).border_color(theme().border_strong));
                },
            ],
        ] }
    }).disabled(move || disabled)
}

pub fn compact_switch(on: Binding<bool>, disabled: impl Fn() -> bool + 'static) -> Div {
    let disabled: std::rc::Rc<dyn Fn() -> bool> = std::rc::Rc::new(disabled);
    let track_disabled = disabled.clone();
    switch_with(on, move |state| {
        let progress = follow(
            move || if state.on() { 1.0_f32 } else { 0.0_f32 },
            Transition::ms(120).ease_out(),
        );
        el! { .div.{
            relative; flex_none; w: 28.0_f32; h: 16.0_f32; rounded: 8.0_f32;
            dyn_style: move s => ${
                let theme = theme();
                let foreground = if state.on() { theme.accent } else { theme.border_strong };
                s.bg(if track_disabled() { foreground.with_alpha(0.4) } else { foreground })
                    .cursor(if track_disabled() { CursorIcon::Default } else { CursorIcon::Pointer })
                    .outline(if state.focus_visible() { 1.0_f32 } else { 0.0_f32 })
                    .outline_offset(2.0_f32).outline_color(theme.focus_ring)
            };
        }.[
            .div.{absolute; top: 2.0_f32; size: 12.0_f32; rounded: 6.0_f32;
                dyn_style: move s => $(s.left(2.0_f32 + 12.0_f32 * progress.get()).bg(theme().surface));
            },
        ] }
    }).disabled(move || disabled())
}
#[derive(Clone, Copy, Debug)]
pub struct NumberRange {
    pub min: f32,
    pub max: f32,
    pub step: f32,
}
impl NumberRange {
    pub fn quantize(self, value: f32) -> f32 {
        if !value.is_finite() {
            return self.max;
        }
        let value = value.clamp(self.min, self.max);
        (((value - self.min) / self.step).round() * self.step + self.min).clamp(self.min, self.max)
    }
    pub fn equivalent(self, left: f32, right: f32) -> bool {
        left.is_finite() && right.is_finite() && self.quantize(left) == self.quantize(right)
    }
}
pub const PERCENTAGE_RANGE: NumberRange = NumberRange {
    min: 0.0,
    max: 100.0,
    step: 1.0,
};
pub const FONT_SIZE_RANGE: NumberRange = NumberRange {
    min: crate::appearance::typography::MIN_SIZE,
    max: crate::appearance::typography::MAX_SIZE,
    step: 1.0,
};
