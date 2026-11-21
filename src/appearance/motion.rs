//! One application motion preference combined with the desktop's reduced-motion setting.
use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;
use std::time::Duration;

use zgui::reactive::{LocalStorage, RenderEffect, RwSignal, Signal, on_cleanup_local, prelude::*};
use zgui::view::time::{FrameHandle, Timers};
use zgui::vocab::Timestamp;

#[cfg(test)]
mod tests;

pub const INLINE_DURATION_MS: u64 = 140;
pub const WORKSPACE_DURATION_MS: u64 = 160;
pub const WORKSPACE_EASING: &str = "cubic-bezier(0, 0, 0.2, 1)";
const SETTLE_EPSILON: f32 = 1e-4;
const INITIAL_FRAME_STEP: Duration = Duration::from_millis(8);
const MIN_BACKDATE_DURATION: Duration = Duration::from_millis(32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    pub duration_ms: u64,
    pub easing: &'static str,
}

impl Transition {
    pub const fn ms(duration_ms: u64, easing: &'static str) -> Self {
        Self {
            duration_ms,
            easing,
        }
    }

    pub fn css_value(self, reduced: bool) -> String {
        if reduced {
            "0ms".to_owned()
        } else {
            format!("{}ms {}", self.duration_ms, self.easing)
        }
    }
}

pub const INLINE: Transition = Transition::ms(INLINE_DURATION_MS, WORKSPACE_EASING);
pub const WORKSPACE: Transition = Transition::ms(WORKSPACE_DURATION_MS, WORKSPACE_EASING);

/// Standard GDS `ease-out cubic` curve without spring overshoot (`1 - (1 - t)^3`).
#[must_use]
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    pub enabled: bool,
    pub system_reduced: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: true,
            system_reduced: false,
        }
    }
}

impl Policy {
    pub fn reduced(self) -> bool {
        !self.enabled || self.system_reduced
    }

    pub(crate) fn write_css_declarations(self, out: &mut String) {
        let reduced = self.reduced();
        let (fast, inline, normal, slow) = if reduced {
            (0, 0, 0, 0)
        } else {
            (100, INLINE_DURATION_MS, WORKSPACE_DURATION_MS, 240)
        };
        let _ = writeln!(
            out,
            "    --gds-motion-reduced: {};",
            if reduced { 1 } else { 0 }
        );
        let _ = writeln!(out, "    --gds-motion-none: 0ms;");
        let _ = writeln!(out, "    --gds-motion-fast: {fast}ms;");
        let _ = writeln!(out, "    --gds-motion-inline: {inline}ms;");
        let _ = writeln!(out, "    --gds-motion-normal: {normal}ms;");
        let _ = writeln!(out, "    --gds-motion-slow: {slow}ms;");
        if reduced {
            let _ = writeln!(out, "    --zui-motion-duration-fast: 0ms;");
            let _ = writeln!(out, "    --zui-motion-duration-normal: 0ms;");
            let _ = writeln!(out, "    --zui-motion-duration-slow: 0ms;");
            let _ = writeln!(out, "    --zui-motion-duration-slower: 0ms;");
            let _ = writeln!(out, "    --zui-motion-duration-slowest: 0ms;");
        }
        let _ = writeln!(
            out,
            "    --gds-motion-ease-standard: cubic-bezier(0.4, 0, 0.2, 1);"
        );
        let _ = writeln!(out, "    --gds-motion-ease-out: {WORKSPACE_EASING};");
        let _ = writeln!(out, "    --gds-motion-ease-in: cubic-bezier(0.4, 0, 1, 1);");
        let _ = writeln!(out, "    --gds-motion-ease-linear: linear;");
    }
}

pub fn enabled() -> bool {
    crate::app::Gds::current().appearance().motion().enabled()
}

struct Flight {
    from: f32,
    to: f32,
    start: Option<Timestamp>,
    last_frame: Option<Timestamp>,
    pre_stepped: bool,
    pending: Option<FrameHandle>,
    in_flight: bool,
    generation: u64,
}

impl Flight {
    fn stop(&mut self) -> Option<FrameHandle> {
        self.in_flight = false;
        self.pre_stepped = false;
        self.start = None;
        self.last_frame = None;
        self.pending.take()
    }
}

fn arm_frame(
    clock: &Timers,
    state: &Rc<RefCell<Flight>>,
    current: RwSignal<f32, LocalStorage>,
    duration: Duration,
    generation: u64,
) {
    enum Step {
        Ignore,
        Continue(f32),
        Settle(f32),
    }

    let clock_next = clock.clone();
    let state_cb = Rc::clone(state);
    let handle = clock.request_frame(move |at| {
        let step = {
            let mut st = state_cb.borrow_mut();
            if !st.in_flight || st.generation != generation {
                Step::Ignore
            } else {
                st.pending = None;
                st.last_frame = Some(at);
                let backdate = if st.pre_stepped {
                    st.pre_stepped = false;
                    INITIAL_FRAME_STEP * 2
                } else {
                    INITIAL_FRAME_STEP
                };
                let origin = *st.start.get_or_insert_with(|| {
                    if duration >= MIN_BACKDATE_DURATION {
                        Timestamp::from_origin(at.since_origin().saturating_sub(backdate))
                    } else {
                        at
                    }
                });
                let elapsed = at.saturating_since(origin);
                if elapsed >= duration || duration.is_zero() {
                    let target_value = st.to;
                    st.in_flight = false;
                    st.pre_stepped = false;
                    st.start = None;
                    st.last_frame = None;
                    Step::Settle(target_value)
                } else {
                    let raw_t = (elapsed.as_secs_f64() / duration.as_secs_f64()) as f32;
                    let next_value = st.from + (st.to - st.from) * ease_out_cubic(raw_t);
                    Step::Continue(next_value)
                }
            }
        };
        match step {
            Step::Ignore => {}
            Step::Settle(val) => {
                if current.get_untracked() != val {
                    current.set(val);
                }
            }
            Step::Continue(val) => {
                if (current.get_untracked() - val).abs() > 1e-6 {
                    current.set(val);
                }
                arm_frame(&clock_next, &state_cb, current, duration, generation);
            }
        }
    });

    let mut st = state.borrow_mut();
    if st.in_flight && st.generation == generation {
        st.pending = Some(handle);
    }
}

/// Frame-driven `ease-out cubic` target interpolator (`140ms` inline / `160ms` workspace).
///
/// Retargets smoothly from the currently displayed value when reversed mid-flight, snaps
/// immediately when reduced motion is active or no window frame clock is mounted, and schedules
/// zero frame callbacks once settled.
#[must_use]
pub fn use_transition(
    target: Signal<f32, LocalStorage>,
    duration_ms: u64,
) -> Signal<f32, LocalStorage> {
    let initial = target.get_untracked();
    let current = RwSignal::new_local(initial);
    let state = Rc::new(RefCell::new(Flight {
        from: initial,
        to: initial,
        start: None,
        last_frame: None,
        pre_stepped: false,
        pending: None,
        in_flight: false,
        generation: 0,
    }));

    let state_fx = Rc::clone(&state);
    let effect = RenderEffect::new(move |_| {
        let goal = target.get();
        let reduced =
            crate::app::Gds::try_current().is_some_and(|gds| gds.appearance().motion().reduced());
        let displayed = current.get_untracked();

        if reduced || duration_ms == 0 || !goal.is_finite() || !displayed.is_finite() {
            let cancelled = state_fx.borrow_mut().stop();
            drop(cancelled);
            if goal.is_finite() && displayed != goal {
                current.set(goal);
            }
            return;
        }

        if (goal - displayed).abs() <= SETTLE_EPSILON {
            let cancelled = state_fx.borrow_mut().stop();
            drop(cancelled);
            if displayed != goal {
                current.set(goal);
            }
            return;
        }

        let already_heading_there = {
            let st = state_fx.borrow();
            st.in_flight && (st.to - goal).abs() <= SETTLE_EPSILON
        };
        if already_heading_there {
            return;
        }

        let Some(clock) = Timers::current() else {
            let cancelled = state_fx.borrow_mut().stop();
            drop(cancelled);
            if displayed != goal {
                current.set(goal);
            }
            return;
        };

        let duration = Duration::from_millis(duration_ms);
        let (cancelled, generation, immediate_kick) = {
            let mut st = state_fx.borrow_mut();
            let was_in_flight = st.in_flight;
            let prev = st.pending.take();
            st.generation = st.generation.wrapping_add(1);
            let generation = st.generation;
            st.from = displayed;
            st.to = goal;
            st.start = if was_in_flight { st.last_frame } else { None };
            st.in_flight = true;
            let kick = if !was_in_flight && duration >= MIN_BACKDATE_DURATION {
                st.pre_stepped = true;
                let raw_t = (INITIAL_FRAME_STEP.as_secs_f64() / duration.as_secs_f64()) as f32;
                Some(displayed + (goal - displayed) * ease_out_cubic(raw_t))
            } else {
                st.pre_stepped = false;
                None
            };
            (prev, generation, kick)
        };
        drop(cancelled);
        if let Some(val) = immediate_kick
            && (current.get_untracked() - val).abs() > 1e-6
        {
            current.set(val);
        }
        arm_frame(&clock, &state_fx, current, duration, generation);
    });

    on_cleanup_local(move || {
        let cancelled = state.borrow_mut().stop();
        drop(cancelled);
        drop(effect);
    });

    current.into()
}

#[cfg(test)]
mod first_frame_tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use zgui::app::Fonts;
    use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
    use zgui::geom::{Device, DevicePx, Rect, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{LocalStorage, RwSignal, Signal, flush};
    use zgui::render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    };
    use zgui::runtime::AppError;
    use zgui::view;
    use zgui_platform_headless::Harness;

    use super::{WORKSPACE_DURATION_MS, use_transition};
    use crate::app::Gds;
    use crate::service::task_manager::Scope;

    struct NoopSink;

    impl TextureSink for NoopSink {
        fn create_texture(
            &mut self,
            _: TextureId,
            _: Size<i32, Device>,
            _: TextureFormat,
        ) -> Result<(), SinkError> {
            Ok(())
        }

        fn write_texture(
            &mut self,
            _: TextureId,
            _: Rect<i32, Device>,
            _: TextureFormat,
            _: &[u8],
        ) -> Result<(), SinkError> {
            Ok(())
        }

        fn destroy_texture(&mut self, _: TextureId) {}
    }

    struct NoopRenderer {
        target: Option<RenderTarget>,
        sink: NoopSink,
    }

    impl Renderer for NoopRenderer {
        fn capabilities(&self) -> RenderCapabilities {
            RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<RenderTarget> {
            self.target
        }

        fn draw(&mut self, _: &zgui::scene::Scene, _: &zgui::bits::DamageSet) -> FrameOutcome {
            FrameOutcome::Presented(FrameStats::default())
        }

        fn register_external(&mut self, _: ExternalTexture) -> TextureHandle {
            TextureHandle(1)
        }

        fn release_external(&mut self, _: TextureHandle) {}

        fn memory(&self) -> MemoryReport {
            MemoryReport::ZERO
        }

        fn texture_sink(&mut self) -> &mut dyn TextureSink {
            &mut self.sink
        }
    }

    fn noop_renderer(
        _: &Arc<dyn Surface>,
        target: RenderTarget,
    ) -> Result<Box<dyn Renderer>, AppError> {
        Ok(Box::new(NoopRenderer {
            target: Some(target),
            sink: NoopSink,
        }))
    }

    #[test]
    fn first_frame_from_rest_advances_immediately_without_dead_stall() {
        let target_slot: Rc<RefCell<Option<RwSignal<f32, LocalStorage>>>> =
            Rc::new(RefCell::new(None));
        let value_slot: Rc<RefCell<Option<Signal<f32, LocalStorage>>>> =
            Rc::new(RefCell::new(None));

        let target_driver = Rc::clone(&target_slot);
        let value_driver = Rc::clone(&value_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(400.0),
                DevicePx(300.0),
            )));
            harness.settle(16);
            harness.advance(Duration::from_millis(32));
            harness.settle(8);

            let target = target_driver.borrow().expect("target signal");
            let animated = value_driver.borrow().expect("animated signal");
            assert_eq!(animated.get_untracked(), 0.0);

            // Trigger 160ms transition from rest and advance exactly ONE 16ms frame.
            target.set(280.0);
            flush();
            harness.run_for(Duration::from_millis(16), Duration::from_millis(16));
            let first_frame = animated.get_untracked();
            assert!(
                first_frame > 20.0 && first_frame < 120.0,
                "first rendered frame after input must already advance smoothly (no 0.0 stall), got {first_frame}"
            );

            harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
            assert_eq!(animated.get_untracked(), 280.0);

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(400.0, 300.0)
            .run_on(driver, move || {
                let _gds = Gds::install(Scope::default());
                let target = RwSignal::new_local(0.0_f32);
                let animated = use_transition(target.into(), WORKSPACE_DURATION_MS);
                *target_slot.borrow_mut() = Some(target);
                *value_slot.borrow_mut() = Some(animated);
                view! {
                    box(
                        style:width = move || Some(format!("{:.2}px", animated.get())),
                        style:height = "38px"
                    ) {}
                }
            })
            .expect("first-frame motion test ran");
    }
}
