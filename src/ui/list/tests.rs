use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use zgui::app::Fonts;
use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
use zgui::geom::{CssPx, Device, DevicePx, Point, Rect, Size};
use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
use zgui::prelude::*;
use zgui::reactive::{RwSignal, UnsyncCallback};
use zgui::render::{
    ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
    Renderer, TextureHandle,
};
use zgui::runtime::AppError;
use zgui::view::{ScrollBehavior, ScrollTarget};
use zgui::vocab::{Modifiers, PointerAction, PointerButton, PointerEvent, Role, Timestamp};
use zgui_platform_headless::Harness;

use super::{
    DataListStyle, ListGeometry, ListRowProps, ListStyle, ListSurfaceProps,
    VirtualizedListSurfaceProps,
};
use crate::ui::page::{PageTableAreaProps, TABLE_INSET};

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
fn list_style_defaults_to_plain_and_tracks_table_insets() {
    assert_eq!(ListStyle::default(), ListStyle::Plain);

    let geometry = ListGeometry {
        rows: 5,
        row_height: DataListStyle::ROW_HEIGHT,
        header_height: DataListStyle::HEADER_HEIGHT,
        inset_x: TABLE_INSET,
        bottom_inset: DataListStyle::SCROLLBAR_LANE,
        scrollbar_width: DataListStyle::SCROLLBAR_LANE,
    };
    let bounds = Rect::new(
        Point::new(CssPx(20.0), CssPx(50.0)),
        Size::new(CssPx(400.0), CssPx(300.0)),
    );
    let hit = |x: f32, y: f32, offset: f32| {
        geometry.is_blank(
            bounds,
            Point::new(CssPx(bounds.origin.x.0 + x), CssPx(bounds.origin.y.0 + y)),
            CssPx(offset),
        )
    };
    assert!(
        !hit(100.0, 180.0, 0.0),
        "a row retains its own selection behavior"
    );
    assert!(
        hit(10.0, 100.0, 0.0),
        "left TABLE_INSET margin clears selection"
    );
    assert!(
        hit(390.0, 100.0, 0.0),
        "right TABLE_INSET margin clears selection"
    );
    assert!(
        !hit(374.0, 100.0, 0.0),
        "vertical scrollbar lane inside inset keeps selection"
    );
    assert!(
        hit(100.0, 240.0, 0.0),
        "space below the rows clears selection"
    );
    assert!(
        hit(100.0, 180.0, 80.0),
        "the live scroll offset moves the blank boundary"
    );
    assert!(!hit(100.0, 15.0, 80.0), "column headings keep selection");
    assert!(
        !hit(100.0, 290.0, 0.0),
        "the horizontal scrollbar lane keeps selection"
    );
    assert!(
        !hit(100.0, 310.0, 0.0),
        "a press outside this list does not clear it"
    );
}

#[test]
fn headless_list_surface_fires_on_blank_press_for_margins_and_scrolled_tail() {
    let blank_hits = Rc::new(Cell::new(0_usize));
    let blank_hits_view = Rc::clone(&blank_hits);
    let blank_hits_driver = Rc::clone(&blank_hits);
    let scroll_slot: Rc<std::cell::RefCell<Option<NodeRef>>> =
        Rc::new(std::cell::RefCell::new(None));
    let scroll_slot_view = Rc::clone(&scroll_slot);
    let scroll_slot_driver = Rc::clone(&scroll_slot);

    let click_at = |harness: &mut Harness<Box<dyn AppHandler>>, x: f32, y: f32| {
        let pt = Point::new(CssPx(x), CssPx(y));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(pt).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(pt).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(8);
    };

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(400.0),
            DevicePx(300.0),
        )));
        harness.settle(16);

        // Header (y = 15 < 30) and populated row (y = 60 in 30..150) do not clear selection.
        click_at(&mut harness, 100.0, 15.0);
        assert_eq!(blank_hits_driver.get(), 0);
        click_at(&mut harness, 100.0, 60.0);
        assert_eq!(blank_hits_driver.get(), 0);

        // Left & right 18px TABLE_INSET side margins clear selection.
        click_at(&mut harness, 8.0, 60.0);
        assert_eq!(blank_hits_driver.get(), 1);
        click_at(&mut harness, 392.0, 60.0);
        assert_eq!(blank_hits_driver.get(), 2);

        // Space below the 3 rows (y = 180 >= 30 + 3 * 40 = 150) clears selection.
        click_at(&mut harness, 100.0, 180.0);
        assert_eq!(blank_hits_driver.get(), 3);

        // Scroll downward by 60px so y = 110 (>= 150 - 60 = 90) becomes blank space.
        let scroll = scroll_slot_driver.borrow().expect("scroll node_ref");
        scroll.scroll_to(
            ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(60.0))),
            ScrollBehavior::Instant,
        );
        harness.settle(16);
        click_at(&mut harness, 100.0, 110.0);
        assert_eq!(blank_hits_driver.get(), 4);

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(400.0, 300.0)
        .run_on(driver, move || {
            let scroll_ref = NodeRef::new();
            *scroll_slot_view.borrow_mut() = Some(scroll_ref);
            let hits = Rc::clone(&blank_hits_view);
            zgui::view! {
                column(style:width = "400px", style:height = "300px") {
                    ListSurface(
                        scroll_ref = scroll_ref,
                        rows = 3_usize,
                        row_height = 40.0_f32,
                        header_height = 30.0_f32,
                        inset_x = TABLE_INSET,
                        on_blank_press = UnsyncCallback::new(move |()| {
                            hits.set(hits.get() + 1);
                        }),
                    ) {
                        PageTableArea {
                            box(style:height = "30px", style:flex-shrink = "0") {
                                text { "Header" }
                            }
                            scroll(
                                node_ref = scroll_ref,
                                class = "gds-list-surface__scroll",
                            ) {
                                column(style:height = "600px") {
                                    ListRow(index = 0) { text { "Row 0" } }
                                    ListRow(index = 1) { text { "Row 1" } }
                                    ListRow(index = 2) { text { "Row 2" } }
                                }
                            }
                        }
                    }
                }
            }
        })
        .expect("headless ListSurface blank press test ran");
}

#[test]
fn virtualized_list_surface_mounts_only_visible_window_of_rows() {
    let built_indices: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
    let built_ref = Arc::clone(&built_indices);
    let selected_item_seen: Arc<Mutex<Option<(Role, Option<bool>)>>> = Arc::new(Mutex::new(None));
    let selected_ref = Arc::clone(&selected_item_seen);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(400.0),
            DevicePx(200.0),
        )));
        harness.settle(32);

        let surface = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("surface");
        for update in surface.a11y_log() {
            for (_, node) in update.nodes {
                if node.role() == Role::ListItem && node.label() == Some("Row 1") {
                    *selected_ref.lock().expect("lock") = Some((node.role(), node.is_selected()));
                }
            }
        }
        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(400.0, 200.0)
        .run_on(driver, move || {
            let count = RwSignal::new_local(10_000_usize);
            let built = Arc::clone(&built_ref);
            zgui::view! {
                column(style:width = "400px", style:height = "200px") {
                    VirtualizedListSurface(
                        count = count,
                        label = "Files list",
                        row = move |index: usize| {
                            built.lock().expect("lock").push(index);
                            let selected = index == 1;
                            zgui::view! {
                                ListRow(
                                    index = index,
                                    selected = selected,
                                    a11y:label = format!("Row {index}")
                                ) {
                                    text {{format!("Row {index}")}}
                                }
                            }
                        }
                    )
                }
            }
        })
        .expect("virtualized list runs headlessly");

    let built = built_indices.lock().expect("lock");
    assert!(!built.is_empty());
    assert!(
        built.len() < 30,
        "virtualized 10,000-row list in 200px viewport should only build visible rows + overscan, built {}",
        built.len()
    );
    assert_eq!(
        *selected_item_seen.lock().expect("lock"),
        Some((Role::ListItem, Some(true)))
    );
}
