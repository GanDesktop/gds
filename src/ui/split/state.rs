use super::{Changed, DIVIDER_HEIGHT, KEY_CONTEXT, MoveDown, MoveUp};
use gpui_kit::{prelude::*, *};
use std::{cell::Cell, rc::Rc};

struct Bindings;
impl Global for Bindings {}

pub struct State {
    pub(super) focus: FocusHandle,
    preferred: Option<f32>,
    minimum_top: f32,
    minimum_bottom: f32,
    default_bottom: f32,
    pub(super) bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(super) dragging: bool,
}

impl EventEmitter<Changed> for State {}

impl State {
    pub fn new(
        preferred: Option<f32>,
        minimum_top: f32,
        minimum_bottom: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        if cx.try_global::<Bindings>().is_none() {
            cx.bind_keys([
                KeyBinding::new("up", MoveUp, Some(KEY_CONTEXT)),
                KeyBinding::new("down", MoveDown, Some(KEY_CONTEXT)),
            ]);
            cx.set_global(Bindings);
        }
        Self {
            focus: cx.focus_handle(),
            preferred: preferred.filter(|height| height.is_finite() && *height > 0.),
            minimum_top,
            minimum_bottom,
            default_bottom: minimum_bottom,
            bounds: Rc::new(Cell::new(Bounds::default())),
            dragging: false,
        }
    }

    pub fn default_bottom(&mut self, height: f32) {
        self.default_bottom = height;
    }

    pub fn height(&self) -> f32 {
        constrained_height(
            self.preferred,
            self.bounds.get().size.height.into(),
            self.minimum_top,
            self.minimum_bottom,
            self.default_bottom,
        )
    }

    pub(super) fn adjust(&mut self, height: f32, cx: &mut Context<Self>) {
        let available = f32::from(self.bounds.get().size.height);
        if available <= 0. {
            return;
        }
        self.preferred = Some(constrained_height(
            Some(height),
            available,
            self.minimum_top,
            self.minimum_bottom,
            self.default_bottom,
        ));
        cx.notify();
    }

    pub(super) fn commit(&self, cx: &mut Context<Self>) {
        if let Some(height) = self.preferred {
            cx.emit(Changed(height));
        }
    }
}

pub fn constrained_height(
    preferred: Option<f32>,
    available: f32,
    top_min: f32,
    bottom_min: f32,
    default_bottom: f32,
) -> f32 {
    let usable = (available - DIVIDER_HEIGHT).max(0.);
    let top_min = top_min.min(usable / 2.);
    let maximum = (usable - bottom_min).max(top_min);
    preferred
        .filter(|value| value.is_finite())
        .unwrap_or(usable - default_bottom)
        .clamp(top_min, maximum)
}
