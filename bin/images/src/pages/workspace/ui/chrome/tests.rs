use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gds::{
    app::Gds,
    i18n::{Catalog, Language},
    service::{image::source::Origin, task_manager::Supervisor},
    ui::file_inspection::Cache,
};
use zgui::{
    app::Fonts,
    atlas::{SinkError, TextureFormat, TextureId, TextureSink},
    geom::{CssPx, Device, DevicePx, Point, Rect, Size},
    platform::{AppHandler, PlatformError, Surface, SurfaceEvent},
    prelude::*,
    reactive::{Owner, flush},
    render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    },
    runtime::AppError,
    view,
    vocab::{Modifiers, PointerAction, PointerButton, PointerEvent, Role, Timestamp},
};
use zgui_platform_headless::{Harness, OffscreenSurface};

use super::WorkspaceViewProps;
use crate::{
    entities::preferences,
    pages::workspace::{
        CaptureWorkflow,
        ui::{
            ActivePopover, ConfirmKind, InlineEdit, Page, Workspace, commands::Command, inspector,
        },
    },
    shared::{geometry::Size2, i18n},
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

#[derive(Clone, Debug)]
struct LiveNode {
    role: Role,
    label: String,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl LiveNode {
    fn center_x(&self) -> f32 {
        ((self.x0 + self.x1) * 0.5) as f32
    }

    fn center_y(&self) -> f32 {
        ((self.y0 + self.y1) * 0.5) as f32
    }
}

fn compose_affine(outer: [f64; 6], inner: [f64; 6]) -> [f64; 6] {
    [
        outer[0] * inner[0] + outer[2] * inner[1],
        outer[1] * inner[0] + outer[3] * inner[1],
        outer[0] * inner[2] + outer[2] * inner[3],
        outer[1] * inner[2] + outer[3] * inner[3],
        outer[0] * inner[4] + outer[2] * inner[5] + outer[4],
        outer[1] * inner[4] + outer[3] * inner[5] + outer[5],
    ]
}

fn transform_rect(m: [f64; 6], r: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    let corners = [(r.0, r.1), (r.2, r.1), (r.2, r.3), (r.0, r.3)];
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (x, y) in corners {
        let tx = m[0] * x + m[2] * y + m[4];
        let ty = m[1] * x + m[3] * y + m[5];
        min_x = min_x.min(tx);
        max_x = max_x.max(tx);
        min_y = min_y.min(ty);
        max_y = max_y.max(ty);
    }
    (min_x, min_y, max_x, max_y)
}

fn collect_live_nodes(surface: &OffscreenSurface) -> Vec<LiveNode> {
    let mut nodes = HashMap::new();
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
        return Vec::new();
    };
    let mut stack = vec![(root_id, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0])];
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    while let Some((id, world)) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(&id) else {
            continue;
        };
        if node.is_hidden() {
            continue;
        }
        let next_world = match node.transform() {
            Some(t) => compose_affine(world, t.as_coeffs()),
            None => world,
        };
        if let Some(rect) = node.bounds() {
            let (x0, y0, x1, y1) = transform_rect(next_world, (rect.x0, rect.y0, rect.x1, rect.y1));
            out.push(LiveNode {
                role: node.role(),
                label: node.label().unwrap_or_default().to_owned(),
                x0,
                y0,
                x1,
                y1,
            });
        }
        for &child in node.children().iter().rev() {
            stack.push((child, next_world));
        }
    }
    out
}

fn pointer_click(harness: &mut Harness<Box<dyn AppHandler>>, x: f32, y: f32, ms: u64) {
    let pos = Point::new(CssPx(x), CssPx(y));
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Moved,
        event: PointerEvent::mouse(pos),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Pressed,
        event: PointerEvent::mouse(pos).with_button(PointerButton::Primary),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.settle(6);
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Released,
        event: PointerEvent::mouse(pos).with_button(PointerButton::Primary),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms + 40)),
    });
    harness.settle(12);
}

fn pointer_drag(
    harness: &mut Harness<Box<dyn AppHandler>>,
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    ms: u64,
) {
    let start = Point::new(CssPx(x), CssPx(y));
    let end = Point::new(CssPx(x + dx), CssPx(y + dy));
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Moved,
        event: PointerEvent::mouse(start),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Pressed,
        event: PointerEvent::mouse(start).with_button(PointerButton::Primary),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.settle(6);
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Moved,
        event: PointerEvent::mouse(end),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms + 20)),
    });
    harness.settle(6);
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Released,
        event: PointerEvent::mouse(end).with_button(PointerButton::Primary),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms + 40)),
    });
    harness.settle(12);
}

#[test]
fn dragging_across_images_top_bar_controls_suppresses_clicks_while_rename_input_stays_editable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let img_path = temp.path().join("sample.png");
    let config_path = temp.path().join("settings.toml");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([20, 120, 220, 255]))
        .save(&img_path)
        .expect("write fixture png");

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let tasks = supervisor.scope();
    let page_slot: Rc<RefCell<Option<Page>>> = Rc::new(RefCell::new(None));
    let page_view = Rc::clone(&page_slot);
    let page_driver = Rc::clone(&page_slot);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        let surface = harness.platform().offscreens()[0].clone();
        surface.set_a11y_listener(true);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1200.0),
            DevicePx(760.0),
        )));
        harness.settle(16);

        let page = page_driver.borrow().expect("page constructed");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while page.read(|s| s.doc.is_none()) {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out loading fixture image"
            );
            harness.advance(Duration::from_millis(20));
            harness.pump();
            flush();
            harness.settle(4);
            std::thread::yield_now();
        }
        harness.settle(12);

        let nodes = collect_live_nodes(&surface);
        let filmstrip_label = Command::Filmstrip.label();
        let rename_label = Command::Rename.label();
        let filmstrip_btn = nodes
            .iter()
            .find(|n| n.role == Role::Button && n.y0 < 56.0 && n.label == filmstrip_label)
            .expect("filmstrip toolbar button")
            .clone();
        let rename_btn = nodes
            .iter()
            .find(|n| n.role == Role::Button && n.y0 < 56.0 && n.label == rename_label)
            .expect("rename toolbar button")
            .clone();
        let filename_x = (rename_btn.x0 - 80.0) as f32;
        let filename_y = rename_btn.center_y();

        // 1. Dragging on the filename pill must NOT open inline rename.
        pointer_drag(&mut harness, filename_x, filename_y, 18.0, 0.0, 100);
        assert!(
            matches!(page.window().inline_edit.get_untracked(), InlineEdit::None),
            "dragging filename pill must move window instead of starting rename"
        );

        // 2. Dragging on the Rename toolbar button must NOT open inline rename.
        pointer_drag(
            &mut harness,
            rename_btn.center_x(),
            rename_btn.center_y(),
            0.0,
            24.0,
            200,
        );
        assert!(
            matches!(page.window().inline_edit.get_untracked(), InlineEdit::None),
            "dragging from rename button must suppress button click"
        );

        // 3. Dragging on the Filmstrip toggle button must NOT toggle filmstrip state.
        let initial_filmstrip = page.window().prefs.snapshot().filmstrip_open;
        pointer_drag(
            &mut harness,
            filmstrip_btn.center_x(),
            filmstrip_btn.center_y(),
            0.0,
            24.0,
            300,
        );
        assert_eq!(
            page.window().prefs.snapshot().filmstrip_open,
            initial_filmstrip,
            "dragging from filmstrip button must not toggle filmstrip"
        );

        // 4. Clicking the filename pill still opens inline rename, and dragging inside the
        //    rename input preserves inline edit mode without triggering window drag.
        pointer_click(&mut harness, filename_x, filename_y, 500);
        assert!(
            matches!(
                page.window().inline_edit.get_untracked(),
                InlineEdit::Rename { .. }
            ),
            "clicking filename pill must still start inline rename"
        );
        pointer_drag(&mut harness, filename_x, filename_y, 32.0, 0.0, 700);
        assert!(
            matches!(
                page.window().inline_edit.get_untracked(),
                InlineEdit::Rename { .. }
            ),
            "dragging inside rename input must keep rename input open for text selection"
        );

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1200.0, 760.0)
        .run_on(driver, move || {
            let app = Gds::install(tasks.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            let catalog = Catalog::new(Language::English, &[i18n::resource_for(Language::English)])
                .expect("English catalog");
            app.i18n().install_catalog(catalog);
            let startup = preferences::read_startup_from(config_path.clone()).expect("startup");
            let prefs = preferences::Service::install(startup, &tasks);
            let scope = app.tasks().owner_scope("Images drag test");
            let cache = Cache::with_scope(scope.clone(), Default::default());
            let page = Page(StoredValue::new_local(Rc::new(Workspace {
                owner: Owner::current().expect("window owner"),
                scope: scope.clone(),
                cache,
                workflow: CaptureWorkflow {
                    output: None,
                    copy_on_save: false,
                    early_exit: false,
                },
                prefs,
                state: RefCell::default(),
                version: RwSignal::new(0),
                catalog_version: RwSignal::new(0),
                paint: RwSignal::new(0),
                overlay: RwSignal::new(0),
                settings: RwSignal::new_local(false),
                about: RwSignal::new_local(false),
                confirm_open: RwSignal::new_local(false),
                confirm_kind: RwSignal::new_local(ConfirmKind::default()),
                confirm_generation: RwSignal::new_local(0),
                pending: RefCell::default(),
                active_popover: RwSignal::new_local(ActivePopover::default()),
                inline_edit: RwSignal::new_local(InlineEdit::default()),
                custom_sizes: RwSignal::new_local(inspector::CustomSizes::default()),
                fonts: RwSignal::new_local(Vec::new()),
                viewport: NodeRef::new(),
                size: std::cell::Cell::new(Size2::new(1200.0, 760.0)),
                loading: RwSignal::new_local(false),
                busy: RwSignal::new_local(false),
                epoch: std::cell::Cell::new(0),
                cancel: RefCell::new(scope.token().child_token()),
                rendering: std::cell::Cell::new(false),
                queued: std::cell::Cell::new(false),
                clipboard_offer: RefCell::default(),
                clipboard_had_offer: std::cell::Cell::new(false),
                clipboard_probing: std::cell::Cell::new(false),
            })));
            page.load(Origin::LocalFile {
                path: img_path.clone(),
                stamp: None,
            });
            *page_view.borrow_mut() = Some(page);
            view! { WorkspaceView(page = page) }
        })
        .expect("headless images top-bar drag test ran");
}
