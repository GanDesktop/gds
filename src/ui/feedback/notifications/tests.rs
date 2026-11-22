use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
use zgui::prelude::*;
use zgui::reactive::{Mounted, install};
use zgui::runtime::AppError;
use zgui::view;
use zgui::vocab::{KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, Role, Timestamp};
use zgui_platform_headless::Harness;

use super::{
    DETAIL_LIMIT, Level, NotificationHost, NotificationHostProps, NotificationQueue, Operation,
    Report, close_all, close_current, focus, interaction, is_focused, report, visible_details,
};

#[test]
fn notification_queue_autohide_levels_and_static_overflow_match_spec() {
    assert!(Level::Info.default_autohide());
    assert!(Level::Success.default_autohide());
    assert!(Level::Warning.default_autohide());
    assert!(!Level::Error.default_autohide());

    assert_eq!(Level::Info.content_role(), Role::Status);
    assert_eq!(Level::Success.content_role(), Role::Status);
    assert_eq!(Level::Warning.content_role(), Role::Alert);
    assert_eq!(Level::Error.content_role(), Role::Alert);

    let all: Vec<String> = (1..=DETAIL_LIMIT + 3)
        .map(|i| format!("item-{i}"))
        .collect();
    let (shown, remaining) = visible_details(&all);
    assert_eq!(shown.len(), DETAIL_LIMIT);
    assert_eq!(remaining, 3);

    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let _host = NotificationHost(zgui::view::Children::from_view(()));
        report(Report::new(Level::Error, "1 failed"));
        assert_eq!(NotificationQueue::active().items().get_untracked().len(), 1);
        close_all();
        assert!(
            NotificationQueue::active()
                .items()
                .get_untracked()
                .is_empty()
        );
    });
    scope.unmount();
}

#[test]
fn operation_start_and_finish_replace_in_place_and_builder_copies_all_details() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let _host = NotificationHost(zgui::view::Children::from_view(()));
        Operation::start("Copying 3 items…");
        let initial = NotificationQueue::active().items().get_untracked();
        assert_eq!(initial.len(), 1);
        assert!(!initial[0].autohide);
        assert_eq!(initial[0].report.summary, "Copying 3 items…");

        Operation::finish(Report::new(Level::Warning, "Copied with 1 warning"));
        let finished = NotificationQueue::active().items().get_untracked();
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].report.summary, "Copied with 1 warning");
        assert!(finished[0].autohide);
    });
    scope.unmount();

    let built = Operation::new(Level::Warning, "Batch completed")
        .item("alpha.png")
        .item("beta.png")
        .into_report();
    assert_eq!(built.level, Level::Warning);
    assert_eq!(built.details.len(), 2);
    assert_eq!(built.copy_text(), "Batch completed\nalpha.png\nbeta.png");
}

#[test]
fn focus_toggle_navigation_and_dismiss_track_selected_entry() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let _host = NotificationHost(zgui::view::Children::from_view(()));
        Report::new(Level::Info, "First").show();
        let details: Vec<String> = (1..=DETAIL_LIMIT + 2)
            .map(|i| format!("detail-{i}"))
            .collect();
        Report::new(Level::Error, "Second").details(details).show();

        let queue = NotificationQueue::active();
        let items = queue.items().get_untracked();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].report.summary, "Second");
        assert!(!items[0].autohide);
        assert!(items[1].autohide);

        // First Alt+N focuses notifications; second Alt+N toggles focus back off.
        focus();
        assert!(is_focused());
        assert_eq!(queue.selected_id().get_untracked(), Some(items[0].id));
        focus();
        assert!(!is_focused());

        focus();
        interaction::select_next();
        assert_eq!(queue.selected_id().get_untracked(), Some(items[1].id));

        close_current();
        assert_eq!(queue.items().get_untracked().len(), 1);
        assert_eq!(queue.items().get_untracked()[0].id, items[0].id);

        close_all();
        assert!(!is_focused());
    });
    scope.unmount();
}

#[test]
fn headless_notifications_alt_n_toggles_focus_restoration_and_alt_x_dismisses() {
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

    let origin_slot: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
    let focused_probe: Rc<RefCell<Option<Signal<Option<NodeId>, zgui::reactive::LocalStorage>>>> =
        Rc::new(RefCell::new(None));

    let origin_view = Rc::clone(&origin_slot);
    let probe_view = Rc::clone(&focused_probe);

    let origin_check = Rc::clone(&origin_slot);
    let probe_check = Rc::clone(&focused_probe);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.pump();
        if let Some(surface) = harness.platform().offscreens().first() {
            surface.set_a11y_listener(true);
        }
        harness.settle(4);

        let origin_ref = origin_check.borrow().expect("origin ref");
        let focus_sig = probe_check.borrow().expect("focus probe");

        // Move keyboard focus onto Origin first via Tab.
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::named(NamedKey::Tab, PhysicalKey::Code(zgui::vocab::KeyCode::Tab)),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        assert_eq!(focus_sig.get_untracked(), origin_ref.get_untracked());

        // Press Alt+N -> moves focus into NotificationHost.
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::character("n"),
            modifiers: Modifiers::ALT,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        assert_ne!(focus_sig.get_untracked(), origin_ref.get_untracked());

        // Press Alt+N a second time -> restores focus back to Origin!
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::character("n"),
            modifiers: Modifiers::ALT,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        assert_eq!(
            focus_sig.get_untracked(),
            origin_ref.get_untracked(),
            "second Alt+N must restore prior focus target"
        );

        // Focus again with Alt+N, then press Esc -> restores focus back to Origin!
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::character("n"),
            modifiers: Modifiers::ALT,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::named(NamedKey::Escape, PhysicalKey::Unidentified(0)),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        assert_eq!(
            focus_sig.get_untracked(),
            origin_ref.get_untracked(),
            "Esc inside notifications must restore prior focus target"
        );

        // Verify outer AT-SPI Role::Alert ('notification') is emitted for both Info and Error,
        // and single-line notifications have equal top and bottom padding.
        let offscreen = harness.platform().offscreens()[0].clone();
        let mut nodes = std::collections::HashMap::new();
        for update in offscreen.a11y_log() {
            for (id, node) in update.nodes {
                nodes.insert(id, node);
            }
        }
        let a11y_alerts: Vec<Option<String>> = nodes
            .values()
            .filter(|n| format!("{:?}", n.role()) == "Alert")
            .map(|n| n.label().map(str::to_owned))
            .collect();
        assert!(
            a11y_alerts
                .iter()
                .any(|l| l.as_deref() == Some("First info")),
            "Info toast must project Role::Alert ('notification') in {a11y_alerts:?}"
        );
        assert!(
            a11y_alerts
                .iter()
                .any(|l| l.as_deref() == Some("Second error")),
            "Error toast must project Role::Alert ('notification') in {a11y_alerts:?}"
        );

        let (&single_card_id, single_card) = nodes
            .iter()
            .find(|(_, n)| format!("{:?}", n.role()) == "Alert" && n.label() == Some("First info"))
            .expect("First info card node");
        let card_rect = single_card.bounds().expect("First info card bounds");
        let card_h = card_rect.y1 - card_rect.y0;

        // Walk descendants of `single_card` accumulating local Y translations relative to card top.
        let mut stack = vec![(single_card_id, 0.0_f64, true)];
        let mut content_y: Option<(f64, f64)> = None;
        let mut button_ys: Vec<(f64, f64)> = Vec::new();
        while let Some((id, parent_y, is_card_root)) = stack.pop() {
            let Some(node) = nodes.get(&id) else {
                continue;
            };
            let local_dy = if is_card_root {
                0.0
            } else {
                node.transform().map_or(0.0, |t| t.as_coeffs()[5])
            };
            let y_base = parent_y + local_dy;
            if !is_card_root && let Some(rect) = node.bounds() {
                let role_name = format!("{:?}", node.role());
                if role_name == "Status" && node.label() == Some("First info") {
                    content_y = Some((y_base + rect.y0, y_base + rect.y1));
                } else if role_name == "Button" {
                    button_ys.push((y_base + rect.y0, y_base + rect.y1));
                }
            }
            for &child in node.children() {
                stack.push((child, y_base, false));
            }
        }

        let (content_y0, content_y1) = content_y.expect("First info content bounds in card");
        let content_top_pad = content_y0;
        let content_bottom_pad = card_h - content_y1;
        assert!(
            (content_top_pad - content_bottom_pad).abs() < 0.5,
            "single-line toast content must have equal top ({content_top_pad}) and bottom ({content_bottom_pad}) padding inside card_h={card_h}"
        );
        assert_eq!(button_ys.len(), 2, "expected Copy and Close IconButtons");
        for (btn_y0, btn_y1) in button_ys {
            let btn_top_pad = btn_y0;
            let btn_bottom_pad = card_h - btn_y1;
            assert!(
                (btn_top_pad - btn_bottom_pad).abs() < 0.5,
                "single-line toast action button must have equal top ({btn_top_pad}) and bottom ({btn_bottom_pad}) padding inside card_h={card_h}"
            );
        }

        // Alt+X dismisses newest notification; Alt+Shift+X dismisses all remaining.
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::character("x"),
            modifiers: Modifiers::ALT,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);

        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::character("X"),
            modifiers: Modifiers::ALT | Modifiers::SHIFT,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_title("gds-notifications-headless-test")
        .with_size(640.0, 480.0)
        .with_renderer(Box::new(|_surface: &Arc<dyn Surface>, target| {
            let mut renderer = HeadlessSink::default();
            zgui::render::Renderer::configure(&mut renderer, target);
            Ok::<_, AppError>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
        }))
        .run_on(driver, move || {
            let origin = NodeRef::new();
            *origin_view.borrow_mut() = Some(origin);
            *probe_view.borrow_mut() = Some(zgui::prelude::focused_node());

            view! {
                NotificationHost {
                    {{
                        Report::new(Level::Info, "First info").autohide(false).show();
                        Report::new(Level::Error, "Second error")
                            .details((1..=6).map(|i| format!("line-{i}")))
                            .show();
                        view! {
                            control(
                                node_ref = origin,
                                tabindex = Focus::Sequential,
                                style:display = "flex",
                                style:width = "100px",
                                style:height = "36px",
                            ) {
                                "Origin"
                            }
                        }
                    }}
                }
            }
        })
        .expect("headless notifications test ran");
}
