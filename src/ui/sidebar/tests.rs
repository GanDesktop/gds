use std::sync::{Arc, Mutex};
use std::time::Duration;

use zgui::app::Fonts;
use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
use zgui::geom::{Device, DevicePx, Rect, Size};
use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, flush, on_cleanup_local};
use zgui::render::{
    ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
    Renderer, TextureHandle,
};
use zgui::runtime::AppError;
use zgui::view;
use zgui::vocab::Role;
use zgui_platform_headless::Harness;

use super::{
    SideBarContentProps, SideBarProps, SideBarSectionContentProps, SideBarSectionHeaderProps,
    SideBarSectionProps, SideBarSectionTitleProps, SidebarPresentation, SidebarState,
    use_sidebar_presentation,
};
use crate::appearance::tokens;

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
fn sidebar_presentation_interpolates_progress_cleanly() {
    let collapsed = SidebarPresentation::from_width(
        tokens::COLLAPSED_SIDEBAR_WIDTH,
        tokens::SIDEBAR_WIDTH,
        tokens::COLLAPSED_SIDEBAR_WIDTH,
    );
    assert_eq!(collapsed.progress(), 0.0);
    assert!(collapsed.is_collapsed());
    assert!(collapsed.collapsed());
    assert!(!collapsed.actions_visible());
    assert_eq!(collapsed.actions_opacity(), 0.0);
    assert_eq!(collapsed.interpolate(100.0, 20.0), 20.0);

    let midpoint = SidebarPresentation::from_width(
        (tokens::SIDEBAR_WIDTH + tokens::COLLAPSED_SIDEBAR_WIDTH) / 2.0,
        tokens::SIDEBAR_WIDTH,
        tokens::COLLAPSED_SIDEBAR_WIDTH,
    );
    assert!((midpoint.progress() - 0.5).abs() < 1e-4);
    assert!(!midpoint.actions_visible());
    assert!((midpoint.label_opacity() - 0.5).abs() < 1e-4);
    assert!((midpoint.actions_opacity() - 0.5).abs() < 1e-4);

    let expanded = SidebarPresentation::from_state(SidebarState::Expanded);
    assert_eq!(expanded.width(), tokens::SIDEBAR_WIDTH);
    assert_eq!(expanded.progress(), 1.0);
    assert!(expanded.actions_visible());
}

#[test]
fn use_sidebar_presentation_animates_continuously_and_settles_at_rail_width() {
    let latest_width: Arc<Mutex<Option<f32>>> = Arc::new(Mutex::new(None));
    let recorded = Arc::clone(&latest_width);
    let state_slot: Arc<Mutex<Option<RwSignal<SidebarState, zgui::reactive::LocalStorage>>>> =
        Arc::new(Mutex::new(None));
    let toggle_slot = Arc::clone(&state_slot);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(800.0),
            DevicePx(600.0),
        )));
        harness.settle(32);

        let expanded_w = *latest_width.lock().expect("lock");
        assert_eq!(
            expanded_w,
            Some(tokens::SIDEBAR_WIDTH),
            "expanded sidebar should lay out at {}px",
            tokens::SIDEBAR_WIDTH
        );

        let surface = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("surface");
        let has_complementary = surface
            .a11y_log()
            .into_iter()
            .flat_map(|update| update.nodes)
            .any(|(_, node)| {
                node.role() == Role::Complementary && node.label() == Some("Workspace sidebar")
            });
        assert!(has_complementary);

        if let Some(sig) = *toggle_slot.lock().expect("lock") {
            sig.set(SidebarState::Collapsed);
            flush();
        }
        // Halfway through 160ms transition, width must be strictly between 56px and 224px.
        harness.run_for(Duration::from_millis(80), Duration::from_millis(16));
        let mid_w = latest_width.lock().expect("lock").expect("mid width");
        assert!(
            mid_w > tokens::COLLAPSED_SIDEBAR_WIDTH && mid_w < tokens::SIDEBAR_WIDTH,
            "sidebar should interpolate continuously mid-flight, got {mid_w}"
        );

        // Settle at 56px rail width and verify zero further frames.
        harness.run_for(Duration::from_millis(220), Duration::from_millis(16));
        let collapsed_w = *latest_width.lock().expect("lock");
        assert_eq!(
            collapsed_w,
            Some(tokens::COLLAPSED_SIDEBAR_WIDTH),
            "collapsed sidebar should settle at {}px rail width",
            tokens::COLLAPSED_SIDEBAR_WIDTH
        );

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(800.0, 600.0)
        .run_on(driver, move || {
            let state = RwSignal::new_local(SidebarState::Expanded);
            *state_slot.lock().expect("lock") = Some(state);
            let presentation = use_sidebar_presentation(state.into());
            let measure_ref = NodeRef::new();
            let border_size = measure_ref.observe_border_size();
            let recorded = Arc::clone(&recorded);
            let effect = RenderEffect::new(move |_| {
                if let Some(bounds) = border_size.get() {
                    let s = measure_ref.scale().max(1.0);
                    *recorded.lock().expect("lock") = Some(bounds.width.0 / s);
                }
            });
            on_cleanup_local(move || drop(effect));

            view! {
                row(style:width = "100%", style:height = "100%") {
                    SideBar(presentation = presentation, label = "Workspace sidebar") {
                        box(node_ref = measure_ref, style:width = "100%", style:height = "1px") {}
                        SideBarContent {
                            SideBarSection(style:padding = "0 10px") {
                                SideBarSectionHeader {
                                    SideBarSectionTitle { "Places" }
                                }
                                SideBarSectionContent {
                                    text { "Home" }
                                }
                            }
                        }
                    }
                }
            }
        })
        .expect("sidebar runs headlessly");
}
