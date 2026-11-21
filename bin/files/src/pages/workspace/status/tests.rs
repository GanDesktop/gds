use std::{
    cell::RefCell,
    collections::HashMap,
    path::Path,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use gds::{app::Gds, i18n::Language, service::task_manager::Scope, ui::IconName};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{
    Layer,
    layer::{Context, SubscriberExt},
};
use zgui::{
    app::Fonts,
    atlas::{SinkError, TextureFormat, TextureId, TextureSink},
    geom::{Device, DevicePx, Rect, Size},
    platform::{AppHandler, PlatformError, Surface, SurfaceEvent},
    prelude::*,
    reactive::{RwSignal, flush},
    render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    },
    runtime::AppError,
    vocab::Role,
};
use zgui_platform_headless::{Harness, OffscreenSurface};

use super::{
    ClipboardDetailState, Kind, Summary, install_status_stylesheets, should_collapse_status_badge,
};
use crate::{
    entities::{
        location::{places::Places, volumes},
        preferences,
    },
    pages::workspace::ui::{Browser, LaunchOptions},
    shared::i18n,
};

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

fn count_live_buttons(surface: &OffscreenSurface) -> usize {
    let mut nodes = std::collections::HashMap::new();
    let mut root = None;
    for update in surface.a11y_log() {
        if let Some(tree) = update.tree {
            root = Some(tree.root);
        }
        for (id, node) in update.nodes {
            nodes.insert(id, node);
        }
    }
    let Some(root_id) = root else {
        return 0;
    };
    let mut stack = vec![root_id];
    let mut seen = std::collections::HashSet::new();
    let mut count = 0;
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(&id) else {
            continue;
        };
        if node.role() == Role::Button {
            count += 1;
        }
        stack.extend(node.children().iter().copied());
    }
    count
}

fn run_status_harness(dir: &Path, check: impl FnOnce(&mut Harness<Box<dyn AppHandler>>, &Browser)) {
    let start = dir.to_path_buf();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let check_slot = RefCell::new(Some(check));

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        if let Some(offscreen) = harness.platform().offscreens().first() {
            offscreen.set_a11y_listener(true);
        }
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(16);
        let browser = browser_slot.borrow().clone().expect("browser constructed");
        let run_check = check_slot.borrow_mut().take().expect("check closure");
        run_check(&mut harness, &browser);
        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope = Scope::default();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            app.i18n()
                .install_catalog(i18n::prepare(Language::English).expect("English catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
                .expect("preferences service");
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(Places::default()),
                    start: start.clone(),
                    home: start.clone(),
                    picker: None,
                },
                volumes::Service::new(scope),
                Rc::new(|_| {}),
            );
            *slot_for_view.borrow_mut() = Some(browser.clone());
            browser.status_controls()
        })
        .expect("headless status controls harness");
}

#[test]
fn status_badge_collapses_when_second_control_appears_or_window_is_compact() {
    assert!(!should_collapse_status_badge(false, 0));
    assert!(!should_collapse_status_badge(false, 1));
    assert!(should_collapse_status_badge(false, 2));
    assert!(should_collapse_status_badge(false, 3));
    assert!(should_collapse_status_badge(true, 1));
}

#[test]
fn background_tasks_summary_tracks_concurrent_tasks_and_cancels_independently() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let temp = tempfile::tempdir().expect("tempdir");

    run_status_harness(temp.path(), |harness, browser| {
        let surface = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("offscreen surface");
        assert_eq!(count_live_buttons(&surface), 0);

        let idle = browser.status_summary(Kind::Tasks);
        assert_eq!(idle.icon, IconName::ListTodo);
        assert_eq!(idle.count, 0);
        assert!(idle.label.is_empty());

        let files_token = browser
            .begin_background_task("files", "Copying 3 items…")
            .expect("files task started");
        flush();
        harness.settle(8);
        assert_eq!(count_live_buttons(&surface), 1);

        let single = browser.status_summary(Kind::Tasks);
        assert_eq!(single.icon, IconName::ListTodo);
        assert_eq!(single.count, 1);
        assert_eq!(single.label, "Copying 3 items…");

        let trash_token = browser
            .begin_background_task("trash", "Emptying Trash…")
            .expect("trash task started");
        flush();
        harness.settle(8);

        let multi = browser.status_summary(Kind::Tasks);
        assert_eq!(multi.icon, IconName::ListTodo);
        assert_eq!(multi.count, 2);
        assert_eq!(multi.label, "2 tasks");

        assert!(browser.cancel_background_task("files"));
        assert!(files_token.is_cancelled());
        assert!(!trash_token.is_cancelled());
    });
}

struct ObserveWarnCounter(Arc<AtomicUsize>);

impl<S: Subscriber> Layer<S> for ObserveWarnCounter {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        if *meta.level() == Level::WARN && meta.target() == "zgui::observe" {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn button_width(surface: &OffscreenSurface) -> Option<f64> {
    let mut nodes = HashMap::new();
    for update in surface.a11y_log() {
        for (id, node) in update.nodes {
            nodes.insert(id, node);
        }
    }
    nodes
        .values()
        .find(|node| node.role() == Role::Button)
        .and_then(|node| node.bounds().map(|bounds| bounds.x1 - bounds.x0))
}

#[test]
fn status_label_transition_settles_without_geometry_observation_warning() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let temp = tempfile::tempdir().expect("tempdir");

    let observe_warns = Arc::new(AtomicUsize::new(0));
    let subscriber =
        tracing_subscriber::registry().with(ObserveWarnCounter(Arc::clone(&observe_warns)));

    let start = temp.path().to_path_buf();
    let expanded_slot: Rc<RefCell<Option<RwSignal<f32, LocalStorage>>>> =
        Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&expanded_slot);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        let offscreen = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("offscreen surface");
        offscreen.set_a11y_listener(true);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(8);
        let full_width = button_width(&offscreen).expect("expanded status button width");
        assert!(
            full_width > f64::from(super::CONTROL_SIZE + super::LABEL_TRAILING_GAP) + 20.0,
            "expected real measured label width at expanded=1.0, got {full_width}"
        );

        let expanded = expanded_slot.borrow().expect("expanded signal constructed");
        let mut half_width = 0.0;
        for progress in [0.75_f32, 0.5, 0.25, 0.5, 0.75, 1.0] {
            expanded.set(progress);
            flush();
            harness.settle(4);
            if (progress - 0.5).abs() < 1e-4 {
                half_width = button_width(&offscreen).expect("mid-transition status button width");
            }
        }
        let expected_half =
            f64::from(super::CONTROL_SIZE) + (full_width - f64::from(super::CONTROL_SIZE)) * 0.5;
        assert!(
            (half_width - expected_half).abs() <= 4.0,
            "expected half-expanded button width near {expected_half:.1}px, got {half_width:.1}px \
             (full={full_width:.1}px)"
        );
        let final_width = button_width(&offscreen).expect("final expanded status button width");
        assert!(
            (final_width - full_width).abs() <= 1.0,
            "expected button to return to {full_width:.1}px, got {final_width:.1}px"
        );
        harness.shut_down();
        Ok(())
    };

    tracing::subscriber::with_default(subscriber, || {
        zgui::app()
            .with_fonts(Fonts::system())
            .with_renderer(Box::new(noop_renderer))
            .with_size(1280.0, 800.0)
            .run_on(driver, move || {
                let scope = Scope::default();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                app.i18n()
                    .install_catalog(i18n::prepare(Language::English).expect("English catalog"));
                let _prefs =
                    preferences::Service::install(preferences::Preferences::default(), &scope)
                        .expect("preferences service");
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(Places::default()),
                        start: start.clone(),
                        home: start.clone(),
                        picker: None,
                    },
                    volumes::Service::new(scope),
                    Rc::new(|_| {}),
                );
                install_status_stylesheets();
                let expanded = RwSignal::new_local(1.0_f32);
                *slot_for_view.borrow_mut() = Some(expanded);
                let summary = Signal::stored_local(Summary {
                    icon: IconName::ListTodo,
                    count: 1,
                    label: "Moving 3 items to Trash…".to_owned(),
                });
                let open = Signal::stored_local(false);
                let clipboard_state = ClipboardDetailState {
                    icons: RwSignal::new_local(HashMap::new()),
                    removing: RwSignal::new_local(false),
                };
                browser.status_control_mounted(
                    Kind::Tasks,
                    summary,
                    expanded.into(),
                    open,
                    clipboard_state,
                )
            })
            .expect("headless status label transition harness");
    });

    assert_eq!(
        observe_warns.load(Ordering::Relaxed),
        0,
        "collapsing/expanding status label must not trigger zgui::observe non-settlement warnings"
    );
}
