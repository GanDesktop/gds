use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zgui::app::Fonts;
use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
use zgui::geom::{Css, CssPx, Device, DevicePx, Point, Rect, Size};
use zgui::platform::{
    AppHandler, DecorationSource, Decorations, PlatformError, Surface, SurfaceEvent,
};
use zgui::prelude::*;
use zgui::reactive::UnsyncCallback;
use zgui::render::{
    ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
    Renderer, TextureHandle,
};
use zgui::runtime::AppError;
use zgui::view;
use zgui::vocab::{Modifiers, PointerAction, PointerButton, PointerEvent, Role, Timestamp};
use zgui_platform_headless::Harness;

use super::{DragAction, record_drag_actions, session_hides_controls, workspace_options};
use crate::app::Gds;
use crate::service::task_manager::Scope;
use crate::ui::field::InputFieldProps;
use crate::ui::frame::{FrameProps, ToolbarProps, WindowDragAreaProps, WorkspaceFrameProps};

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
fn workspace_options_use_transparent_csd_defaults() {
    let options = workspace_options("GDS Test", Some("io.github.GanDesktop.Test"));
    let attrs = options.attributes();
    assert_eq!(attrs.decorations, Decorations::None);
    assert!(attrs.transparent);
    assert_eq!(attrs.title, "GDS Test");
    assert_eq!(
        attrs.application_id.as_deref(),
        Some("io.github.GanDesktop.Test")
    );
}

#[test]
fn niri_session_hides_the_entire_control_group() {
    for desktop in ["niri", "Niri", "niri:GNOME", " custom : niri "] {
        assert!(session_hides_controls(Some(desktop), false));
    }
}

#[test]
fn declared_desktop_takes_precedence_over_inherited_niri_socket() {
    for desktop in ["GNOME", "KDE", "sway", "not-niri"] {
        assert!(!session_hides_controls(Some(desktop), true));
    }
}

#[test]
fn missing_desktop_uses_the_niri_socket_as_a_fallback() {
    assert!(session_hides_controls(None, true));
    assert!(session_hides_controls(Some(" "), true));
    assert!(!session_hides_controls(None, false));
}

#[test]
fn csd_controls_respect_capabilities_callbacks_and_server_decorations() {
    let found_close: Arc<Mutex<Option<(Role, Option<String>, bool)>>> = Arc::new(Mutex::new(None));
    let found_max: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let custom_closed = Rc::new(Cell::new(0usize));
    let custom_maximized = Rc::new(Cell::new(0usize));

    let recorded = Arc::clone(&found_close);
    let recorded_max = Arc::clone(&found_max);
    let close_cb_ref = Rc::clone(&custom_closed);
    let max_cb_ref = Rc::clone(&custom_maximized);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(640.0),
            DevicePx(120.0),
        )));
        harness.settle(32);

        let surface = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("window surface created");
        harness
            .platform()
            .set_surface_window_capabilities(surface.id(), Some(true), Some(true));
        harness.settle(16);

        for update in surface.a11y_log() {
            for (_, node) in update.nodes {
                if node.role() == Role::Button && node.label() == Some("Close window") {
                    *recorded.lock().expect("lock") = Some((
                        node.role(),
                        node.label().map(str::to_owned),
                        node.toggled().is_some(),
                    ));
                }
                if node.role() == Role::Button && node.label() == Some("Maximize window") {
                    *recorded_max.lock().expect("lock") = true;
                }
            }
        }

        // Click maximize button (left of close button: right-inset 12 + close 32 + gap 4 + center 16 = 64 -> x = 576).
        let max_pos = Point::new(CssPx(576.0), CssPx(26.0));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(max_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(max_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(16);

        // Click close button (x = 612, y = 26).
        let close_pos = Point::new(CssPx(612.0), CssPx(26.0));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(close_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(close_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(16);

        // Negotiate server decorations dynamically -> controls hide so clicking close_pos does nothing!
        harness
            .platform()
            .set_surface_decoration_source(surface.id(), Some(DecorationSource::Platform));
        harness.settle(16);
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(close_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(close_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(16);

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_decorations(Decorations::None)
        .with_size(640.0, 120.0)
        .run_on(driver, move || {
            let close_cb = Rc::clone(&close_cb_ref);
            let max_cb = Rc::clone(&max_cb_ref);
            super::title_bar_with_forced_controls(
                "Test Window",
                Some(UnsyncCallback::new(move |()| {
                    max_cb.set(max_cb.get() + 1);
                })),
                Some(UnsyncCallback::new(move |()| {
                    close_cb.set(close_cb.get() + 1);
                })),
            )
        })
        .expect("window titlebar runs headlessly");

    assert_eq!(
        *found_close.lock().expect("lock"),
        Some((Role::Button, Some("Close window".to_owned()), false)),
        "CSD close button must project Role::Button with label 'Close window' and no toggled state"
    );
    assert!(
        *found_max.lock().expect("lock"),
        "CSD maximize button must mount when can_maximize is true"
    );
    assert_eq!(custom_maximized.get(), 1);
    assert_eq!(
        custom_closed.get(),
        1,
        "close callback fires under CSD and is suppressed once server_decorated becomes true"
    );
}

fn pointer_press(
    harness: &mut Harness<Box<dyn AppHandler>>,
    pos: Point<CssPx, Css>,
    button: PointerButton,
    ms: u64,
) {
    let ts = Timestamp::from_origin(Duration::from_millis(ms));
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Moved,
        event: PointerEvent::mouse(pos),
        modifiers: Modifiers::NONE,
        timestamp: ts,
    });
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Pressed,
        event: PointerEvent::mouse(pos).with_button(button),
        modifiers: Modifiers::NONE,
        timestamp: ts,
    });
    harness.settle(8);
}

fn pointer_move(harness: &mut Harness<Box<dyn AppHandler>>, pos: Point<CssPx, Css>, ms: u64) {
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Moved,
        event: PointerEvent::mouse(pos),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.settle(8);
}

fn pointer_release(
    harness: &mut Harness<Box<dyn AppHandler>>,
    pos: Point<CssPx, Css>,
    button: PointerButton,
    ms: u64,
) {
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Released,
        event: PointerEvent::mouse(pos).with_button(button),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.settle(8);
}

fn node_center(node: NodeRef) -> Point<CssPx, Css> {
    let bounds = node.window_bounds().expect("node bounds");
    let scale = node.scale().max(1.0);
    Point::new(
        CssPx((bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale),
        CssPx((bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale),
    )
}

#[test]
fn entire_top_bar_drags_after_threshold_while_preserving_clicks_and_text_inputs() {
    let actions = record_drag_actions();
    let menu_clicks = Rc::new(Cell::new(0usize));
    let action_clicks = Rc::new(Cell::new(0usize));
    let pill_clicks = Rc::new(Cell::new(0usize));
    let close_clicks = Rc::new(Cell::new(0usize));

    let menu_ref_slot: Arc<Mutex<Option<NodeRef>>> = Arc::new(Mutex::new(None));
    let action_ref_slot: Arc<Mutex<Option<NodeRef>>> = Arc::new(Mutex::new(None));
    let pill_ref_slot: Arc<Mutex<Option<NodeRef>>> = Arc::new(Mutex::new(None));
    let input_ref_slot: Arc<Mutex<Option<NodeRef>>> = Arc::new(Mutex::new(None));
    let spacer_ref_slot: Arc<Mutex<Option<NodeRef>>> = Arc::new(Mutex::new(None));

    let menu_driver = Arc::clone(&menu_ref_slot);
    let action_driver = Arc::clone(&action_ref_slot);
    let pill_driver = Arc::clone(&pill_ref_slot);
    let input_driver = Arc::clone(&input_ref_slot);
    let spacer_driver = Arc::clone(&spacer_ref_slot);

    let menu_clicks_view = Rc::clone(&menu_clicks);
    let action_clicks_view = Rc::clone(&action_clicks);
    let pill_clicks_view = Rc::clone(&pill_clicks);
    let close_clicks_view = Rc::clone(&close_clicks);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(960.0),
            DevicePx(300.0),
        )));
        harness.settle(32);

        let surface = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("surface");
        harness
            .platform()
            .set_surface_window_capabilities(surface.id(), Some(true), Some(true));
        harness.settle(16);

        let menu_pt = node_center(menu_driver.lock().expect("lock").expect("menu ref"));
        let action_pt = node_center(action_driver.lock().expect("lock").expect("action ref"));
        let pill_pt = node_center(pill_driver.lock().expect("lock").expect("pill ref"));
        let input_pt = node_center(input_driver.lock().expect("lock").expect("input ref"));
        let spacer_pt = node_center(spacer_driver.lock().expect("lock").expect("spacer ref"));
        let close_pt = Point::new(CssPx(932.0), CssPx(24.0));

        // 1. Click with sub-threshold jitter on menu button activates it without dragging.
        pointer_press(&mut harness, menu_pt, PointerButton::Primary, 100);
        pointer_move(
            &mut harness,
            Point::new(CssPx(menu_pt.x.0 + 2.0), CssPx(menu_pt.y.0 + 1.0)),
            110,
        );
        pointer_release(
            &mut harness,
            Point::new(CssPx(menu_pt.x.0 + 2.0), CssPx(menu_pt.y.0 + 1.0)),
            PointerButton::Primary,
            120,
        );
        assert_eq!(actions.take(), Vec::<DragAction>::new());

        // 2. Dragging downward out of the toolbar from the menu button starts a move drag and suppresses click.
        pointer_press(&mut harness, menu_pt, PointerButton::Primary, 200);
        let below_bar = Point::new(CssPx(menu_pt.x.0), CssPx(72.0));
        pointer_move(&mut harness, below_bar, 220);
        pointer_release(&mut harness, below_bar, PointerButton::Primary, 240);
        assert_eq!(actions.take(), vec![DragAction::MoveDrag]);

        // 3. Dragging from a toolbar action button starts a move drag and suppresses its click.
        pointer_press(&mut harness, action_pt, PointerButton::Primary, 300);
        let action_drag = Point::new(CssPx(action_pt.x.0 + 14.0), CssPx(action_pt.y.0));
        pointer_move(&mut harness, action_drag, 320);
        pointer_release(&mut harness, action_drag, PointerButton::Primary, 340);
        assert_eq!(actions.take(), vec![DragAction::MoveDrag]);

        // 4. Rapid double-click on an interactive button activates it twice and never maximizes.
        pointer_press(&mut harness, action_pt, PointerButton::Primary, 500);
        pointer_release(&mut harness, action_pt, PointerButton::Primary, 520);
        pointer_press(&mut harness, action_pt, PointerButton::Primary, 560);
        pointer_release(&mut harness, action_pt, PointerButton::Primary, 580);
        assert_eq!(actions.take(), Vec::<DragAction>::new());

        // 5. Non-editing clickable title/address pill clicks normally, and drags when moved past threshold.
        pointer_press(&mut harness, pill_pt, PointerButton::Primary, 700);
        pointer_release(&mut harness, pill_pt, PointerButton::Primary, 720);
        assert_eq!(actions.take(), Vec::<DragAction>::new());

        pointer_press(&mut harness, pill_pt, PointerButton::Primary, 1200);
        let pill_drag = Point::new(CssPx(pill_pt.x.0 + 16.0), CssPx(pill_pt.y.0));
        pointer_move(&mut harness, pill_drag, 1220);
        pointer_release(&mut harness, pill_drag, PointerButton::Primary, 1240);
        assert_eq!(actions.take(), vec![DragAction::MoveDrag]);

        // 6. Dragging inside a text entry input never starts a window drag.
        pointer_press(&mut harness, input_pt, PointerButton::Primary, 1400);
        let input_drag = Point::new(CssPx(input_pt.x.0 + 24.0), CssPx(input_pt.y.0));
        pointer_move(&mut harness, input_drag, 1420);
        pointer_release(&mut harness, input_drag, PointerButton::Primary, 1440);
        assert_eq!(actions.take(), Vec::<DragAction>::new());

        // 7. Empty toolbar drag strip supports drag, double-press maximize, and secondary window menu.
        pointer_press(&mut harness, spacer_pt, PointerButton::Primary, 1600);
        let spacer_drag = Point::new(CssPx(spacer_pt.x.0 + 12.0), CssPx(spacer_pt.y.0));
        pointer_move(&mut harness, spacer_drag, 1620);
        pointer_release(&mut harness, spacer_drag, PointerButton::Primary, 1640);
        assert_eq!(actions.take(), vec![DragAction::MoveDrag]);

        pointer_press(&mut harness, spacer_pt, PointerButton::Primary, 2100);
        pointer_release(&mut harness, spacer_pt, PointerButton::Primary, 2120);
        pointer_press(&mut harness, spacer_pt, PointerButton::Primary, 2200);
        pointer_release(&mut harness, spacer_pt, PointerButton::Primary, 2220);
        assert_eq!(actions.take(), vec![DragAction::DoubleClick]);

        pointer_press(&mut harness, spacer_pt, PointerButton::Secondary, 2400);
        pointer_release(&mut harness, spacer_pt, PointerButton::Secondary, 2420);
        assert_eq!(actions.take(), vec![DragAction::WindowMenu(spacer_pt)]);

        // Secondary press on interactive button does not open the native window menu.
        pointer_press(&mut harness, action_pt, PointerButton::Secondary, 2500);
        pointer_release(&mut harness, action_pt, PointerButton::Secondary, 2520);
        assert_eq!(actions.take(), Vec::<DragAction>::new());

        // 8. Window control button drags when moved past threshold and still clicks on release.
        pointer_press(&mut harness, close_pt, PointerButton::Primary, 2700);
        let close_drag = Point::new(CssPx(close_pt.x.0 - 14.0), CssPx(close_pt.y.0));
        pointer_move(&mut harness, close_drag, 2720);
        pointer_release(&mut harness, close_drag, PointerButton::Primary, 2740);
        assert_eq!(actions.take(), vec![DragAction::MoveDrag]);

        pointer_press(&mut harness, close_pt, PointerButton::Primary, 3000);
        pointer_release(&mut harness, close_pt, PointerButton::Primary, 3020);
        assert_eq!(actions.take(), Vec::<DragAction>::new());

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_decorations(Decorations::None)
        .with_size(960.0, 300.0)
        .run_on(driver, move || {
            let _gds = Gds::install(Scope::default());
            let menu_ref = NodeRef::new();
            let action_ref = NodeRef::new();
            let pill_ref = NodeRef::new();
            let input_ref = NodeRef::new();
            let spacer_ref = NodeRef::new();
            *menu_ref_slot.lock().expect("lock") = Some(menu_ref);
            *action_ref_slot.lock().expect("lock") = Some(action_ref);
            *pill_ref_slot.lock().expect("lock") = Some(pill_ref);
            *input_ref_slot.lock().expect("lock") = Some(input_ref);
            *spacer_ref_slot.lock().expect("lock") = Some(spacer_ref);

            let menu_c = Rc::clone(&menu_clicks_view);
            let action_c = Rc::clone(&action_clicks_view);
            let pill_c = Rc::clone(&pill_clicks_view);
            let close_c = Rc::clone(&close_clicks_view);

            let menu_btn = AnyView::new(view! {
                control(
                    node_ref = menu_ref,
                    tabindex = Focus::Sequential,
                    style:width = "32px",
                    style:height = "32px",
                    on:click = move |_| menu_c.set(menu_c.get() + 1)
                ) {"M"}
            });
            let toolbar_controls = AnyView::new(view! {
                row(style:display = "flex", style:align_items = "center", style:gap = "8px", style:flex = "1 1 0%") {
                    control(
                        node_ref = action_ref,
                        tabindex = Focus::Sequential,
                        style:width = "32px",
                        style:height = "32px",
                        on:click = move |_| action_c.set(action_c.get() + 1)
                    ) {"A"}
                    control(
                        node_ref = pill_ref,
                        style:width = "140px",
                        style:height = "32px",
                        on:click = move |_| pill_c.set(pill_c.get() + 1)
                    ) {"location.png"}
                    box(style:width = "180px") {
                        InputField(
                            node_ref = input_ref,
                            default_value = "rename-me.png",
                            label = "Rename",
                            on:pointer_down = |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerDown>| {
                                ev.stop_propagation();
                            },
                        )
                    }
                    WindowDragArea {
                        box(node_ref = spacer_ref, style:flex = "1 1 0%", style:height = "32px")
                    }
                }
            });
            let window_controls = super::render_window_controls(
                Classes::default(),
                false,
                None,
                Some(UnsyncCallback::new(move |()| {
                    close_c.set(close_c.get() + 1);
                })),
            );
            let sidebar_body = AnyView::new(view! { box { text {"Nav"} } });
            let main_body = AnyView::new(view! { box { text {"Body"} } });

            view! {
                Frame() {
                    Toolbar(
                        menu = menu_btn,
                        controls = toolbar_controls,
                        window_controls = window_controls,
                    )
                    WorkspaceFrame(
                        sidebar = sidebar_body,
                        content = main_body,
                        paint_content_surface = true
                    )
                }
            }
        })
        .expect("entire top-bar drag test ran");

    assert_eq!(menu_clicks.get(), 1, "only sub-threshold click fires menu");
    assert_eq!(
        action_clicks.get(),
        2,
        "drag suppresses click while rapid double-click still fires twice"
    );
    assert_eq!(
        pill_clicks.get(),
        1,
        "only non-drag click activates title/address pill"
    );
    assert_eq!(
        close_clicks.get(),
        1,
        "only non-drag click activates window close button"
    );
}
