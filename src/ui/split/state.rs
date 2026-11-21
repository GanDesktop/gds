use zgui::reactive::{LocalStorage, RwSignal, Signal, prelude::*};

use super::DIVIDER_HEIGHT;

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub min: f32,
    pub max: f32,
    pub initial: f32,
    pub collapse_below: Option<f32>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            min: 90.0,
            max: 600.0,
            initial: 220.0,
            collapse_below: None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct State {
    width: RwSignal<f32, LocalStorage>,
    collapsed: RwSignal<bool, LocalStorage>,
    options: Options,
}

impl State {
    #[must_use]
    pub fn new(options: Options) -> Self {
        let lo = options.min.max(0.0);
        let hi = options.max.max(lo);
        let initial = if options.initial.is_finite() {
            options.initial.clamp(lo, hi)
        } else {
            lo
        };
        Self {
            width: RwSignal::new_local(initial),
            collapsed: RwSignal::new_local(false),
            options,
        }
    }

    #[must_use]
    pub fn options(self) -> Options {
        self.options
    }

    #[must_use]
    pub fn width(self) -> Signal<f32, LocalStorage> {
        self.width.into()
    }

    pub fn set_width(self, width: f32) {
        if !width.is_finite() {
            return;
        }
        let (lo, hi) = self.effective_bounds(None);
        self.width.set(width.clamp(lo, hi));
    }

    #[must_use]
    pub fn collapsed(self) -> Signal<bool, LocalStorage> {
        self.collapsed.into()
    }

    pub fn set_collapsed(self, collapsed: bool) {
        self.collapsed.set(collapsed);
    }

    /// Resize toward `target` (CSS pixels), snapping to collapsed when below `collapse_below`
    /// and clamping inside `[min, max]` and `max_available`.
    pub fn resize_to(self, target: f32, max_available: Option<f32>) {
        if !target.is_finite() {
            return;
        }
        if let Some(threshold) = self.options.collapse_below
            && target < threshold
        {
            self.collapsed.set(true);
            return;
        }
        let (lo, hi) = self.effective_bounds(max_available);
        self.collapsed.set(false);
        self.width.set(target.clamp(lo, hi));
    }

    /// Adjust the split by `delta` CSS pixels, reopening from collapsed when `delta > 0`.
    pub fn step_by(self, delta: f32, max_available: Option<f32>) {
        if !delta.is_finite() {
            return;
        }
        if self.collapsed.get_untracked() {
            if delta > 0.0 {
                let target = self
                    .options
                    .collapse_below
                    .map_or(self.options.min, |t| t.max(self.options.min))
                    .max(delta);
                self.resize_to(target, max_available);
            }
            return;
        }
        self.resize_to(self.width.get_untracked() + delta, max_available);
    }

    /// Snap to the smallest uncollapsed pane size.
    pub fn snap_min(self, max_available: Option<f32>) {
        let (lo, _) = self.effective_bounds(max_available);
        self.collapsed.set(false);
        self.width.set(lo);
    }

    /// Snap to the largest uncollapsed pane size within `max_available`.
    pub fn snap_max(self, max_available: Option<f32>) {
        let (_, hi) = self.effective_bounds(max_available);
        self.collapsed.set(false);
        self.width.set(hi);
    }

    /// Toggle the collapsed state while preserving the preferred uncollapsed size.
    pub fn toggle_collapsed(self) {
        self.collapsed.update(|c| *c = !*c);
    }

    /// Effective committed size (`0.0` when collapsed, otherwise the current clamped size).
    #[must_use]
    pub fn commit_size(self) -> f32 {
        if self.collapsed.get_untracked() {
            0.0
        } else {
            self.width.get_untracked()
        }
    }

    fn effective_bounds(self, max_available: Option<f32>) -> (f32, f32) {
        let lo = self.options.min.max(0.0);
        let hi = self.options.max.max(lo);
        if let Some(avail) = max_available
            && avail.is_finite()
            && avail > 0.0
        {
            let eff_lo = lo.min(avail);
            let eff_hi = hi.min(avail).max(eff_lo);
            (eff_lo, eff_hi)
        } else {
            (lo, hi)
        }
    }
}

#[must_use]
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
