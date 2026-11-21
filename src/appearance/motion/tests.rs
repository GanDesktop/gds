use std::sync::Arc;
use std::time::Duration;

use zgui::app::Fonts;
use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
use zgui::geom::{Device, DevicePx, Rect, Size};
use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Mounted, RwSignal, Signal, flush, install};
use zgui::render::{
    ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
    Renderer, TextureHandle,
};
use zgui::runtime::AppError;
use zgui::view;
use zgui_platform_headless::Harness;

use super::{
    INLINE, INLINE_DURATION_MS, Policy, WORKSPACE, WORKSPACE_DURATION_MS, ease_out_cubic,
    use_transition,
};
use crate::app::Gds;
use crate::service::task_manager::Scope;

struct NoopSink;

impl TextureSink for NoopSink {
    fn create_texture(
        &mut self,
        _: TextureId,
        _: zgui::geom::Size<i32, Device>,
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
fn preference_off_on_restores_system_and_late_system_updates_cannot_enable() {
    let mut policy = Policy::default();
    for system in [false, true] {
        policy.system_reduced = system;
        policy.enabled = false;
        assert!(policy.reduced());
        policy.system_reduced = !system;
        assert!(policy.reduced());
        policy.enabled = true;
        assert_eq!(policy.reduced(), !system);
        policy.enabled = false;
        policy.system_reduced = system;
        policy.enabled = true;
        assert_eq!(policy.reduced(), system);
    }
}

#[test]
fn reactive_motion_zeroes_durations_in_css_when_reduced() {
    install().expect("install reactive runtime");
    let root = Mounted::new();
    root.with(|| {
        let gds = Gds::install(Scope::default());
        let motion = gds.appearance().motion();
        assert!(motion.enabled());
        assert_eq!(INLINE.css_value(false), "140ms cubic-bezier(0, 0, 0.2, 1)");
        assert_eq!(INLINE.css_value(true), "0ms");
        assert_eq!(
            WORKSPACE.css_value(false),
            "160ms cubic-bezier(0, 0, 0.2, 1)"
        );
        assert_eq!(WORKSPACE.css_value(true), "0ms");
        assert!((ease_out_cubic(0.5) - 0.875).abs() < 1e-6);

        let css = gds.appearance().gds_stylesheet_css();
        assert!(css.contains("--gds-motion-inline: 140ms;"));
        assert!(css.contains("--gds-motion-normal: 160ms;"));
        assert!(!css.contains("--zui-motion-duration-slow: 0ms;"));
        assert_eq!(
            gds.appearance().tokens_theme().motion.duration_normal,
            "160ms"
        );

        motion.set_enabled(false);
        assert!(!motion.enabled());
        let css = gds.appearance().gds_stylesheet_css();
        assert!(css.contains("--gds-motion-reduced: 1;"));
        assert!(css.contains("--gds-motion-fast: 0ms;"));
        assert!(css.contains("--gds-motion-inline: 0ms;"));
        assert!(css.contains("--gds-motion-normal: 0ms;"));
        assert!(css.contains("--gds-motion-slow: 0ms;"));
        assert!(css.contains("--zui-motion-duration-fast: 0ms;"));
        assert!(css.contains("--zui-motion-duration-normal: 0ms;"));
        assert!(css.contains("--zui-motion-duration-slow: 0ms;"));
        assert!(css.contains("--zui-motion-duration-slower: 0ms;"));
        assert!(css.contains("--zui-motion-duration-slowest: 0ms;"));
        assert_eq!(
            gds.appearance().tokens_theme().motion.duration_normal,
            "0ms"
        );
        assert_eq!(gds.appearance().tokens_theme().motion.duration_slow, "0ms");
    });
    root.unmount();
}

#[test]
fn use_transition_interpolates_retargets_snaps_when_reduced_and_idles_when_settled() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let target_slot: Rc<RefCell<Option<RwSignal<f32, LocalStorage>>>> = Rc::new(RefCell::new(None));
    let value_slot: Rc<RefCell<Option<Signal<f32, LocalStorage>>>> = Rc::new(RefCell::new(None));
    let gds_slot: Rc<RefCell<Option<Gds>>> = Rc::new(RefCell::new(None));

    let target_driver = Rc::clone(&target_slot);
    let value_driver = Rc::clone(&value_slot);
    let gds_driver = Rc::clone(&gds_slot);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(400.0),
            DevicePx(300.0),
        )));
        harness.settle(16);

        let target = target_driver.borrow().expect("target signal");
        let animated = value_driver.borrow().expect("animated signal");
        let gds = gds_driver.borrow().clone().expect("gds");

        // Initial settled state schedules zero frames!
        assert_eq!(animated.get_untracked(), 58.0);
        harness.reset_counts();
        let idle_frames = harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
        assert_eq!(
            idle_frames, 0,
            "settled initial state must request zero frames"
        );

        // Start 160ms transition 58.0 -> 222.0 and advance halfway (~80ms).
        target.set(222.0);
        flush();
        harness.run_for(Duration::from_millis(80), Duration::from_millis(16));
        let mid = animated.get_untracked();
        assert!(
            mid > 120.0 && mid < 220.0,
            "mid-flight value should follow ease-out cubic between 58 and 222, got {mid}"
        );

        // Reverse mid-flight back to 58.0: must retarget smoothly from `mid` without jumping!
        target.set(58.0);
        flush();
        assert!(
            (animated.get_untracked() - mid).abs() < 1e-4,
            "retargeting mid-flight must keep current displayed value before next frame"
        );
        harness.run_for(Duration::from_millis(16), Duration::from_millis(16));
        let after_reverse = animated.get_untracked();
        assert!(
            after_reverse < mid && after_reverse > 58.0,
            "first frame after reversal must move smoothly toward 58 from {mid}, got {after_reverse}"
        );

        // Complete transition and verify zero extra frames across a full 2s maintenance window.
        harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
        assert_eq!(animated.get_untracked(), 58.0);
        harness.reset_counts();
        let post_settle_frames = harness.run_for(Duration::from_secs(2), Duration::from_millis(16));
        assert_eq!(
            post_settle_frames, 0,
            "settled transition must schedule zero frames"
        );
        assert_eq!(harness.parked_deadline(), None);

        // Enabling reduced motion snaps immediately on flush without multi-frame animation.
        gds.appearance().motion().set_enabled(false);
        flush();
        harness.settle(8);
        harness.reset_counts();
        target.set(222.0);
        flush();
        assert_eq!(
            animated.get_untracked(),
            222.0,
            "reduced motion must snap target immediately"
        );
        // Drain the single DOM style:width redraw for the snapped 222px value, then
        // verify zero animation frames are scheduled across the 200ms window.
        assert_eq!(harness.pump(), 1);
        harness.reset_counts();
        let reduced_frames = harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
        assert_eq!(reduced_frames, 0);

        // Re-enable motion and test 140ms inline duration constant is wired.
        assert_eq!(INLINE_DURATION_MS, 140);
        assert_eq!(WORKSPACE_DURATION_MS, 160);

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(400.0, 300.0)
        .run_on(driver, move || {
            let gds = Gds::install(Scope::default());
            *gds_slot.borrow_mut() = Some(gds);
            let target = RwSignal::new_local(58.0_f32);
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
        .expect("headless motion test ran");
}
