use zgui::prelude::*;
use zgui::reactive::{Mounted, RwSignal, UnsyncCallback, install};
use zgui::view;

use super::{DisclosureIcon, DisclosureIconProps, DisclosureProps};

#[test]
fn disclosure_symbol_is_exported() {
    let _ = DisclosureIcon;
}

#[test]
fn disclosure_and_icon_build_uncontrolled_and_controlled() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let open = RwSignal::new_local(false);
        let disabled = RwSignal::new_local(false);

        let _icon = view! {
            DisclosureIcon(open = open)
        };

        let _controlled = view! {
            Disclosure(
                open = open,
                disabled = disabled,
                show_icon = true,
                on_open_change = UnsyncCallback::new(move |next| open.set(next)),
                header = || AnyView::new(view! { text {"Details"} }),
                content = || AnyView::new(view! { text {"Hidden payload"} }),
            )
        };

        let _uncontrolled = view! {
            Disclosure(
                default_open = true,
                header = || AnyView::new(view! { text {"Summary"} }),
                content = || AnyView::new(view! { text {"Body"} }),
            )
        };

        open.set(true);
        assert!(open.get_untracked());
    });
    scope.unmount();
}

#[test]
fn headless_disclosure_click_toggles_open_and_updates_expanded_a11y() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use zgui::geom::Point;
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::runtime::AppError;
    use zgui::vocab::{Modifiers, PointerAction, PointerButton, PointerEvent, Timestamp};
    use zgui_platform_headless::Harness;

    #[derive(Debug, Default)]
    struct HeadlessSink {
        atlas: zgui::atlas::MemorySink,
        target: Option<zgui::render::RenderTarget>,
    }

    impl zgui::render::Renderer for HeadlessSink {
        fn capabilities(&self) -> zgui::render::RenderCapabilities {
            zgui::render::RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: zgui::render::RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<zgui::render::RenderTarget> {
            self.target
        }

        fn register_external(
            &mut self,
            _texture: zgui::render::ExternalTexture,
        ) -> zgui::render::TextureHandle {
            zgui::render::TextureHandle(0)
        }

        fn release_external(&mut self, _handle: zgui::render::TextureHandle) {}

        fn memory(&self) -> zgui::render::MemoryReport {
            zgui::render::MemoryReport::default()
        }

        fn draw(
            &mut self,
            _scene: &zgui::scene::Scene,
            _damage: &zgui::bits::DamageSet,
        ) -> zgui::render::FrameOutcome {
            zgui::render::FrameOutcome::Presented(zgui::render::FrameStats::default())
        }

        fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
            &mut self.atlas
        }
    }

    let events: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let hdr_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
    let events_view = Rc::clone(&events);
    let hdr_slot_view = Rc::clone(&hdr_slot);
    let events_check = Rc::clone(&events);
    let hdr_slot_check = Rc::clone(&hdr_slot);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.pump();
        if let Some(surface) = harness.platform().offscreens().first() {
            surface.set_a11y_listener(true);
        }
        harness.settle(4);

        let click_header = |harness: &mut Harness<Box<dyn AppHandler>>| {
            let node = hdr_slot_check.borrow().expect("header ref bound");
            let bounds = node.window_bounds().expect("header bounds laid out");
            let scale = node.scale().max(1.0);
            let cx = (bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale;
            let cy = (bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale;
            let at = Point::new(zgui::geom::CssPx(cx), zgui::geom::CssPx(cy));
            for (action, event) in [
                (PointerAction::Moved, PointerEvent::mouse(at)),
                (
                    PointerAction::Pressed,
                    PointerEvent::mouse(at).with_button(PointerButton::Primary),
                ),
                (
                    PointerAction::Released,
                    PointerEvent::mouse(at).with_button(PointerButton::Primary),
                ),
            ] {
                harness.deliver_to_first(SurfaceEvent::Pointer {
                    action,
                    event,
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            harness.settle(4);
        };

        click_header(&mut harness);
        assert_eq!(*events_check.borrow(), vec![true]);

        click_header(&mut harness);
        assert_eq!(*events_check.borrow(), vec![true, false]);

        let offscreen = harness.platform().offscreens()[0].clone();
        let expanded_states: Vec<Option<bool>> = offscreen
            .a11y_log()
            .iter()
            .flat_map(|u| u.nodes.iter())
            .filter(|(_, n)| format!("{:?}", n.role()) == "Button")
            .map(|(_, n)| n.is_expanded())
            .collect();
        assert!(
            expanded_states.contains(&Some(true)) && expanded_states.contains(&Some(false)),
            "expected expanded states true and false in {expanded_states:?}"
        );

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_title("disclosure-test")
        .with_size(400.0, 240.0)
        .with_renderer(Box::new(|_surface: &Arc<dyn Surface>, target| {
            let mut renderer = HeadlessSink::default();
            zgui::render::Renderer::configure(&mut renderer, target);
            Ok::<_, AppError>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
        }))
        .run_on(driver, move || {
            let hdr_ref = NodeRef::new();
            *hdr_slot_view.borrow_mut() = Some(hdr_ref);
            let on_change = Rc::clone(&events_view);
            view! {
                Disclosure(
                    show_icon = true,
                    on_open_change = UnsyncCallback::new(move |next| on_change.borrow_mut().push(next)),
                    header = move || AnyView::new(view! { text(node_ref = hdr_ref) {"Details"} }),
                    content = || AnyView::new(view! { text {"Hidden payload"} }),
                )
            }
        })
        .expect("headless disclosure test ran");
}
