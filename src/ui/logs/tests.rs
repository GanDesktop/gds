use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use zgui::app::app;
use zgui::geom::{DevicePx, Point};
use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
use zgui::prelude::{GetUntracked, Set, WithUntracked};
use zgui::view::{ScrollBehavior, ScrollTarget};
use zgui::vocab::{Key, Modifiers};
use zgui_platform_headless::Harness;

use super::*;
use crate::service::logs::Entry;
use crate::service::task_manager::Supervisor;

mod navigation;
mod tasks;

#[derive(Default)]
struct NullRenderer {
    target: Option<zgui::render::RenderTarget>,
    atlas: zgui::atlas::MemorySink,
}

impl zgui::render::Renderer for NullRenderer {
    fn capabilities(&self) -> zgui::render::RenderCapabilities {
        zgui::render::RenderCapabilities::MINIMAL
    }

    fn configure(&mut self, target: zgui::render::RenderTarget) {
        self.target = Some(target);
    }

    fn target(&self) -> Option<zgui::render::RenderTarget> {
        self.target
    }

    fn draw(
        &mut self,
        _scene: &zgui::scene::Scene,
        _damage: &zgui::bits::DamageSet,
    ) -> zgui::render::FrameOutcome {
        zgui::render::FrameOutcome::Presented(zgui::render::FrameStats {
            vector_passes: 0,
            draw_calls: 0,
            damage_px: 0,
            bytes_uploaded: 0,
            memory: zgui::render::MemoryReport::ZERO,
        })
    }

    fn register_external(
        &mut self,
        _texture: zgui::render::ExternalTexture,
    ) -> zgui::render::TextureHandle {
        zgui::render::TextureHandle(0)
    }

    fn release_external(&mut self, _handle: zgui::render::TextureHandle) {}

    fn memory(&self) -> zgui::render::MemoryReport {
        zgui::render::MemoryReport::ZERO
    }

    fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
        &mut self.atlas
    }
}

pub(super) fn pump_frames(harness: &mut Harness<Box<dyn AppHandler>>) {
    for _ in 0..4 {
        harness.pump();
    }
}

pub(super) fn deliver(harness: &mut Harness<Box<dyn AppHandler>>) {
    harness.pump();
    harness.advance(UPDATE_INTERVAL + Duration::from_millis(5));
    pump_frames(harness);
}

pub(super) fn with_viewer_probe<R>(f: impl FnOnce(&test_probe::ViewerProbe) -> R) -> R {
    test_probe::PROBE.with(|slot| {
        let borrowed = slot.borrow();
        let probe = borrowed.as_ref().expect("log viewer probe mounted");
        f(probe)
    })
}

pub(super) fn run_headless_logs(
    inspect_tasks: bool,
    store: Option<Store>,
    assertions: impl FnOnce(&mut Harness<Box<dyn AppHandler>>, &Gds),
) {
    let supervisor = Supervisor::new(inspect_tasks).expect("test Tokio supervisor");
    let scope = supervisor.scope();
    let gds_slot: Rc<RefCell<Option<Gds>>> = Rc::new(RefCell::new(None));
    let gds_for_ctx = Rc::clone(&gds_slot);
    let store_slot = RefCell::new(store);

    let handler = app()
        .with_title("logs-test")
        .with_application_id("dev.gds.LogsTest")
        .with_size(800.0, 560.0)
        .with_stylesheet(":root { display: flex; width: 100%; height: 100%; }")
        .with_renderer(Box::new(
            move |_surface: &std::sync::Arc<dyn Surface>, target: zgui::render::RenderTarget| {
                let mut renderer = NullRenderer::default();
                zgui::render::Renderer::configure(&mut renderer, target);
                Ok::<_, zgui::Error>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
            },
        ))
        .with_context(move || {
            let log_store = store_slot.borrow_mut().take();
            let gds = Gds::install_with_logs(scope.clone(), log_store);
            gds.appearance()
                .theme()
                .apply(&crate::appearance::theme::Appearance {
                    source: crate::appearance::theme::Source::Builtin,
                    ..crate::appearance::theme::Appearance::default()
                })
                .expect("default test theme");
            *gds_for_ctx.borrow_mut() = Some(gds);
        })
        .into_handler(|| view! { LogViewer() })
        .expect("build headless logs test app");

    handler
        .drive(|app| -> Result<(), PlatformError> {
            let mut harness = Harness::new(app);
            pump_frames(&mut harness);
            harness.deliver_to_first(SurfaceEvent::Focused(true));
            pump_frames(&mut harness);
            let borrowed = gds_slot.borrow();
            let gds = borrowed.as_ref().expect("GDS installed");
            assertions(&mut harness, gds);
            drop(borrowed);
            harness.shut_down();
            Ok(())
        })
        .expect("run headless logs test app");
}

#[test]
fn disabled_logging_has_no_store_window_or_shortcut() {
    run_headless_logs(false, None, |_, gds| {
        assert!(!gds.ui().logs().is_enabled());
        assert!(gds.ui().logs().store().is_none());
        assert!(gds.tasks().inspector().is_none());
    });

    assert!(window::is_open_shortcut(
        &Key::Character("l".into()),
        Modifiers::PRIMARY | Modifiers::SHIFT
    ));
    assert!(!window::is_open_shortcut(
        &Key::Character("l".into()),
        Modifiers::PRIMARY
    ));
}

#[test]
fn store_updates_live_view_and_clear_keeps_collecting() {
    let store = Store::new();
    run_headless_logs(true, Some(store.clone()), |harness, _| {
        store.record(Level::Info, "frontend", "first");
        deliver(harness);
        with_viewer_probe(|probe| {
            assert_eq!(probe.snapshot.with_untracked(|s| s.entries.len()), 1);
            assert_eq!(
                probe
                    .snapshot
                    .with_untracked(|s| s.entries[0].message.clone()),
                "first"
            );
            assert_eq!(probe.visible.get_untracked().len(), 1);
        });

        store.clear();
        deliver(harness);
        assert!(store.snapshot().entries.is_empty());
        with_viewer_probe(|probe| {
            assert!(probe.visible.get_untracked().is_empty());
        });

        store.record(Level::Warn, "frontend", "after clearing");
        deliver(harness);
        with_viewer_probe(|probe| {
            assert_eq!(probe.snapshot.with_untracked(|s| s.entries.len()), 1);
            assert_eq!(
                probe
                    .snapshot
                    .with_untracked(|s| s.entries[0].message.clone()),
                "after clearing"
            );
        });
    });
}

#[test]
fn scrolling_up_stops_following_but_not_log_collection() {
    let store = Store::new();
    run_headless_logs(true, Some(store.clone()), |harness, _| {
        for index in 0..100 {
            store.record(Level::Info, "frontend", format!("entry {index}"));
        }
        deliver(harness);
        with_viewer_probe(|probe| {
            let pos = probe.list_ref.scroll_position();
            assert!(
                pos.is_at_end_vertically(),
                "initial log viewer should tail the newest entries"
            );
            probe.list_ref.scroll_to(
                ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(0.0))),
                ScrollBehavior::Instant,
            );
        });
        pump_frames(harness);

        store.record(Level::Info, "frontend", "arrived while reading");
        deliver(harness);
        with_viewer_probe(|probe| {
            assert_eq!(probe.snapshot.with_untracked(|s| s.entries.len()), 101);
            let pos = probe.list_ref.scroll_position();
            assert!(
                !pos.is_at_end_vertically(),
                "scrolled-up log view must stay pinned to earlier entries"
            );
            let scale = probe.list_ref.scale().max(1.0);
            let bottom = (pos.content_size.height.0 - pos.scrollport.height.0)
                .max(101.0 * ROW_HEIGHT * scale);
            probe.list_ref.scroll_to(
                ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(bottom))),
                ScrollBehavior::Instant,
            );
        });
        pump_frames(harness);

        store.record(Level::Info, "frontend", "follow again");
        deliver(harness);
        with_viewer_probe(|probe| {
            assert_eq!(probe.snapshot.with_untracked(|s| s.entries.len()), 102);
            assert!(
                probe.list_ref.scroll_position().is_at_end_vertically(),
                "returning to bottom must resume auto-tailing"
            );
        });
    });
}

#[test]
fn search_level_and_target_filters_preserve_retained_multiline_text() {
    let store = Store::new();
    run_headless_logs(true, Some(store.clone()), |harness, _| {
        let message = format!(
            "first\r\nsecond\tline\n{}끝",
            "界".repeat(PREVIEW_CHARS * 3)
        );
        let warn_id = store.record(Level::Warn, "Frontend.Network", message.clone());
        store.record(Level::Info, "Frontend.Network", "other");
        store.record(Level::Warn, "other", "unrelated");
        deliver(harness);

        let expected = store.snapshot().entries[0].text();
        assert_eq!(expected.lines().count(), 3);
        assert!(preview_text(&message).ends_with('…'));
        assert!(!preview_text(&message).contains('\n'));

        with_viewer_probe(|probe| {
            probe.query.set("NETWORK".to_owned());
            probe
                .level_key
                .set(filters::level_to_key(Some(Level::Warn)).to_owned());
            probe.selected.set(Some(warn_id));
        });
        pump_frames(harness);

        with_viewer_probe(|probe| {
            let visible = probe.visible.get_untracked();
            assert_eq!(visible, vec![0]);
            assert_eq!(probe.selected.get_untracked(), Some(warn_id));
            probe.target_key.set("other".to_owned());
        });
        pump_frames(harness);

        with_viewer_probe(|probe| {
            assert!(probe.visible.get_untracked().is_empty());
        });
    });

    let sample = Entry {
        id: 1,
        time: SystemTime::UNIX_EPOCH,
        level: Level::Error,
        target: "core".to_owned(),
        message: "boom".to_owned(),
    };
    assert!(
        FilterState {
            level: Some(Level::Error),
            target: Some("core".to_owned()),
            query: "BOOM".to_owned(),
        }
        .matches(&sample)
    );
}

#[test]
fn secondary_log_window_opens_focuses_singleton_and_closes_cleanly() {
    assert_eq!(window::INITIAL_WIDTH, 1_000.0);
    assert_eq!(window::INITIAL_HEIGHT, 720.0);
    assert_eq!(
        window::format_window_title("GDS Files", "Debug"),
        "GDS Files · Debug"
    );
    assert_eq!(window::format_window_title("", "Debug"), "GDS · Debug");
    assert_eq!(toggle_selected_id(None, 42), Some(42));
    assert_eq!(toggle_selected_id(Some(42), 42), None);
    assert_eq!(toggle_selected_id(Some(42), 99), Some(99));

    init(Options::default());
    assert!(is_enabled());
    let shared_store = store().expect("global logs store enabled");
    shared_store.record(Level::Info, "window-test", "boot log");

    let opened_count = Rc::new(std::cell::Cell::new(0usize));
    let opened_for_view = Rc::clone(&opened_count);
    let supervisor = Supervisor::new(false).expect("test Tokio supervisor");
    let scope = supervisor.scope();

    let handler = app()
        .with_title("host-app")
        .with_application_id("dev.gds.LogsWindowTest")
        .with_size(640.0, 480.0)
        .with_stylesheet(":root { display: flex; width: 100%; height: 100%; }")
        .with_renderer(Box::new(
            move |_surface: &std::sync::Arc<dyn Surface>, target: zgui::render::RenderTarget| {
                let mut renderer = NullRenderer::default();
                zgui::render::Renderer::configure(&mut renderer, target);
                Ok::<_, zgui::Error>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
            },
        ))
        .with_context(move || {
            let _ = Gds::install_with_logs(scope.clone(), Some(shared_store.clone()));
        })
        .into_handler(move || {
            open();
            open();
            opened_for_view.set(opened_for_view.get() + 1);
            view! { box() }
        })
        .expect("build secondary logs window test app");

    handler
        .drive(|app| -> Result<(), PlatformError> {
            let mut harness = Harness::new(app);
            pump_frames(&mut harness);
            assert_eq!(opened_count.get(), 1);
            window::close_active();
            pump_frames(&mut harness);
            harness.shut_down();
            Ok(())
        })
        .expect("drive secondary logs window test");
}

#[test]
fn clicking_selected_log_row_toggles_selection_off() {
    use zgui::geom::CssPx;
    use zgui::vocab::{PointerAction, PointerButton, PointerEvent, Timestamp};

    let store = Store::new();
    run_headless_logs(false, Some(store.clone()), |harness, _| {
        let id = store.record(Level::Info, "frontend", "clickable log entry");
        deliver(harness);

        let row_pos = with_viewer_probe(|probe| {
            let bounds = probe.list_ref.window_bounds().expect("list bounds");
            let scale = probe.list_ref.scale().max(1.0);
            zgui::geom::Point::new(
                CssPx(bounds.origin.x.0 / scale + 40.0),
                CssPx(bounds.origin.y.0 / scale + ROW_HEIGHT * 0.5),
            )
        });

        for ms in [10_u64, 40_u64] {
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(row_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(row_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::from_origin(Duration::from_millis(ms + 5)),
            });
            pump_frames(harness);
            if ms == 10 {
                with_viewer_probe(|probe| {
                    assert_eq!(probe.selected.get_untracked(), Some(id));
                });
            } else {
                with_viewer_probe(|probe| {
                    assert_eq!(
                        probe.selected.get_untracked(),
                        None,
                        "re-clicking an already-selected log row must toggle selection off"
                    );
                });
            }
        }
    });
}
