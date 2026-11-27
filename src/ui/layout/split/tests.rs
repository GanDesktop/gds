use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use zgui::app::Fonts;
use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
use zgui::geom::{CssPx, Device, DevicePx, Point, Rect, Size};
use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
use zgui::prelude::*;
use zgui::reactive::{Mounted, UnsyncCallback, install};
use zgui::render::{
    ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
    Renderer, TextureHandle,
};
use zgui::runtime::AppError;
use zgui::view;
use zgui::vocab::{
    KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, PointerAction, PointerButton,
    PointerEvent, Role, Timestamp,
};
use zgui_platform_headless::Harness;

use super::{Options, State, VerticalSplitProps, constrained_height};

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
fn shrinking_the_workspace_does_not_replace_the_preferred_height() {
    let preferred = Some(340.);
    assert_eq!(constrained_height(preferred, 260., 90., 100., 180.), 151.);
    assert_eq!(constrained_height(preferred, 700., 90., 100., 180.), 340.);
    let automatic = constrained_height(None, 700., 90., 100., 180.);
    assert_eq!(automatic, 511.);
}

#[test]
fn split_state_clamps_collapses_and_restores_width() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let state = State::new(Options {
            min: 90.0,
            max: 400.0,
            initial: 200.0,
            collapse_below: Some(50.0),
        });
        assert_eq!(state.width().get_untracked(), 200.0);
        assert!(!state.collapsed().get_untracked());

        // Clamping against max and available container height.
        state.resize_to(500.0, Some(350.0));
        assert_eq!(state.width().get_untracked(), 350.0);

        // Resizing below collapse_below snaps collapsed while preserving prior size.
        state.resize_to(40.0, Some(350.0));
        assert!(state.collapsed().get_untracked());
        assert_eq!(state.commit_size(), 0.0);
        assert_eq!(state.width().get_untracked(), 350.0);

        // Reopening via step_by restores uncollapsed state.
        state.step_by(12.0, Some(350.0));
        assert!(!state.collapsed().get_untracked());
        assert_eq!(state.width().get_untracked(), 90.0);

        // Non-finite inputs are ignored.
        state.resize_to(f32::NAN, None);
        assert_eq!(state.width().get_untracked(), 90.0);
    });
    scope.unmount();
}

#[test]
fn vertical_split_pointer_capture_drag_and_keyboard_resize_commit() {
    let committed: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let committed_ref = Arc::clone(&committed);
    let captured_state: Arc<Mutex<Option<State>>> = Arc::new(Mutex::new(None));
    let state_slot = Arc::clone(&captured_state);
    let splitter_found = Rc::new(Cell::new(false));
    let splitter_flag = Rc::clone(&splitter_found);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(600.0),
            DevicePx(500.0),
        )));
        harness.settle(32);

        let surface = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("surface");
        let has_splitter = surface
            .a11y_log()
            .into_iter()
            .flat_map(|update| update.nodes)
            .any(|(_, node)| node.role() == Role::Splitter);
        splitter_flag.set(has_splitter);

        // Divider sits at y = 500 - 200 = 300 in CSS px. Press at y=300, drag up 60px to y=240.
        let down_pos = Point::new(CssPx(200.0), CssPx(300.0));
        let move_pos = Point::new(CssPx(200.0), CssPx(240.0));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(down_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        // Move outside the 9px handle band — pointer capture must keep delivering.
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(move_pos),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(move_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(16);

        // Now press ArrowDown while handle holds focus (moved focus on press).
        let key_down = KeyEvent::named(NamedKey::ArrowDown, PhysicalKey::Code(KeyCode::ArrowDown));
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: key_down.clone(),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Released,
            event: key_down,
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(16);

        // Pressing Enter on SplitHandle must NOT toggle collapse (parity with main!).
        let enter_key = KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter));
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: enter_key.clone(),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Released,
            event: enter_key,
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(16);

        harness.shut_down();
        Ok(())
    };

    let bg_mounted = Rc::new(Cell::new(false));
    let bg_flag = Rc::clone(&bg_mounted);

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(600.0, 500.0)
        .run_on(driver, move || {
            let state = State::new(Options {
                min: 80.0,
                max: 420.0,
                initial: 200.0,
                collapse_below: Some(40.0),
            });
            *state_slot.lock().expect("lock") = Some(state);
            let committed = Arc::clone(&committed_ref);
            let bg_flag = Rc::clone(&bg_flag);
            view! {
                VerticalSplit(
                    state = state,
                    label = "Terminal dock",
                    divider_background = move || {
                        bg_flag.set(true);
                        AnyView::new(view! { box(style:width = "58px", style:height = "1px") {} })
                    },
                    on_commit = UnsyncCallback::new(move |size: f32| {
                        committed.lock().expect("lock").push(size);
                    }),
                    leading = || AnyView::new(view! { box { text { "Top" } } }),
                    trailing = || AnyView::new(view! { box { text { "Bottom" } } })
                )
            }
        })
        .expect("split runs headlessly");

    assert!(
        splitter_found.get(),
        "SplitHandle must project Role::Splitter"
    );
    assert!(
        bg_mounted.get(),
        "VerticalSplit must render custom divider_background inside the 1px divider slot"
    );
    let commits = committed.lock().expect("lock").clone();
    assert_eq!(commits, vec![260.0, 248.0]);
}

#[test]
fn vertical_split_interpolates_bottom_progress_collapsed_height_and_maximized_progress() {
    let bottom_h: Arc<Mutex<Option<f32>>> = Arc::new(Mutex::new(None));
    let bottom_h_out = Arc::clone(&bottom_h);
    let prog_slot: Arc<Mutex<Option<zgui::reactive::RwSignal<f32, zgui::reactive::LocalStorage>>>> =
        Arc::new(Mutex::new(None));
    let max_slot: Arc<Mutex<Option<zgui::reactive::RwSignal<f32, zgui::reactive::LocalStorage>>>> =
        Arc::new(Mutex::new(None));
    let prog_driver = Arc::clone(&prog_slot);
    let max_driver = Arc::clone(&max_slot);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(600.0),
            DevicePx(500.0),
        )));
        harness.settle(32);

        // Initial progress = 0.0 -> collapsed_bottom_height = 36.0!
        assert_eq!(*bottom_h_out.lock().expect("lock"), Some(36.0));

        // Open to progress = 1.0 -> height = 200.0!
        if let Some(sig) = *prog_driver.lock().expect("lock") {
            sig.set(1.0);
            zgui::reactive::flush();
        }
        harness.settle(32);
        assert_eq!(*bottom_h_out.lock().expect("lock"), Some(200.0));

        // Maximize bottom pane -> height = 500.0 (full container height)!
        if let Some(sig) = *max_driver.lock().expect("lock") {
            sig.set(1.0);
            zgui::reactive::flush();
        }
        harness.settle(32);
        assert_eq!(*bottom_h_out.lock().expect("lock"), Some(500.0));

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(600.0, 500.0)
        .run_on(driver, move || {
            let state = State::new(Options {
                min: 80.0,
                max: 420.0,
                initial: 200.0,
                collapse_below: None,
            });
            let progress = zgui::reactive::RwSignal::new_local(0.0_f32);
            let maximized = zgui::reactive::RwSignal::new_local(0.0_f32);
            *prog_slot.lock().expect("lock") = Some(progress);
            *max_slot.lock().expect("lock") = Some(maximized);

            let trailing_ref = NodeRef::new();
            let trailing_sz = trailing_ref.observe_border_size();
            let out = Arc::clone(&bottom_h);
            let fx = zgui::reactive::RenderEffect::new(move |_| {
                if let Some(sz) = trailing_sz.get() {
                    let s = trailing_ref.scale().max(1.0);
                    *out.lock().expect("lock") = Some(sz.height.0 / s);
                }
            });
            zgui::reactive::on_cleanup_local(move || drop(fx));

            view! {
                VerticalSplit(
                    state = state,
                    bottom_progress = Signal::from(progress),
                    collapsed_bottom_height = 36.0,
                    bottom_maximized_progress = Signal::from(maximized),
                    leading = || AnyView::new(view! { box(style:width = "100%", style:height = "100%") {} }),
                    trailing = move || AnyView::new(view! {
                        box(node_ref = trailing_ref, style:width = "100%", style:height = "100%") {}
                    })
                )
            }
        })
        .expect("split progress test runs");
}
