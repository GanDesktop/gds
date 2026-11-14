//! Suite settings presentation: rows, numeric ranges and keyboard-accessible sliders.
//! Preference models, defaults and persistence belong to their applications.

pub mod form;
mod row;

pub use row::*;

use crate::appearance::typography;
use gds_uim::el;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState, SliderValue};
use gpui_kit::{prelude::*, *};
use std::cell::Cell;

pub const CONTROL_WIDTH: f32 = 260.0;
const RESET_WIDTH: f32 = 32.0;
const CONTROL_GAP: f32 = 8.0;
const NUMBER_LABEL_WIDTH_REMS: f32 = 4.5;

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
    min: typography::MIN_SIZE,
    max: typography::MAX_SIZE,
    step: 1.0,
};

pub struct NumberControl {
    pub state: Entity<SliderState>,
    focus: FocusHandle,
    pub range: NumberRange,
    programmatic_value: Cell<Option<f32>>,
}

impl NumberControl {
    pub fn new<T: 'static>(value: f32, range: NumberRange, cx: &mut Context<T>) -> Self {
        let state = cx.new(|_| {
            SliderState::new()
                .min(range.min)
                .max(range.max)
                .step(range.step)
                .default_value(range.quantize(value))
        });
        Self {
            state,
            focus: cx.focus_handle(),
            range,
            programmatic_value: Cell::new(None),
        }
    }

    /// Keeping equal state untouched also preserves an active drag.
    pub fn sync<T: 'static>(&self, value: f32, window: &mut Window, cx: &mut Context<T>) {
        let value = self.range.quantize(value);
        if self.state.read(cx).value() != SliderValue::Single(value) {
            self.programmatic_value.set(Some(value));
            self.state
                .update(cx, |state, cx| state.set_value(value, window, cx));
        }
    }

    pub fn notification_is_programmatic(&self, value: SliderValue) -> bool {
        self.programmatic_value
            .take()
            .is_some_and(|expected| value == SliderValue::Single(expected))
    }

    fn pointer_focus_handler(
        &self,
        disabled: bool,
    ) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + use<> {
        let focus = self.focus.clone();
        move |_, window, cx| {
            if !disabled {
                focus.focus(window, cx);
            }
        }
    }

    fn key_handler(&self, disabled: bool) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + use<> {
        let state = self.state.clone();
        let range = self.range;
        move |event, window, cx| {
            let modifiers = event.keystroke.modifiers;
            if disabled || modifiers.control || modifiers.alt || modifiers.platform {
                return;
            }
            let current = state.read(cx).value().end();
            let next = match event.keystroke.key.as_str() {
                "left" | "down" => current - range.step,
                "right" | "up" => current + range.step,
                "home" => range.min,
                "end" => range.max,
                _ => return,
            };
            let next = range.quantize(next);
            cx.stop_propagation();
            window.prevent_default();
            if next != current {
                state.update(cx, |state, cx| {
                    state.set_value(next, window, cx);
                    cx.emit(SliderEvent::Change(SliderValue::Single(next)));
                });
            }
        }
    }

    pub fn element(
        &self,
        id: ElementId,
        label: SharedString,
        value_label: SharedString,
        disabled: bool,
    ) -> impl IntoElement + use<> {
        let on_pointer_down = self.pointer_focus_handler(disabled);
        let on_key_down = self.key_handler(disabled);
        el! {
            .div {
                # id;
                test_support;
                track_focus: $(&self.focus);
                tab_index: $(if disabled { -1 } else { 0 }); tab_stop: $(!disabled);
                flex; items_center; gap: CONTROL_GAP.px; w_full;
                on_mouse_down: MouseButton::Left, on_pointer_down;
                on_key_down: on_key_down;
            } [
                $(&self.state).Slider::new {
                    accessibility_label: label; disabled: disabled;
                    flex_1;
                },
                // The value and its unit share one unbreakable line.
                .div {
                    w: $(rems(NUMBER_LABEL_WIDTH_REMS));
                    flex_shrink_0; whitespace_nowrap;
                    text_right; text_sm;
                } [value_label],
            ]
        }
    }
}
