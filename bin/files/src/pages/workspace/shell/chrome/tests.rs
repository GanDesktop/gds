use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gds::{
    app::Gds, i18n::Language, service::task_manager::Scope, ui::sidebar::SidebarPresentation,
};
use zgui::{
    app::Fonts,
    atlas::{SinkError, TextureFormat, TextureId, TextureSink},
    geom::{CssPx, Device, DevicePx, Matrix4, Point, Rect, Size, transformed_bounds},
    platform::{AppHandler, PlatformError, Surface, SurfaceEvent},
    prelude::*,
    render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    },
    runtime::AppError,
    view,
    vocab::{Modifiers, PointerAction, PointerButton, PointerEvent, Role, Timestamp},
};
use zgui_platform_headless::{Harness, OffscreenSurface};

use super::Chrome;
use crate::{
    entities::{
        location::{places::Places, volumes},
        preferences,
    },
    pages::workspace::{shell::Browser, state::LaunchOptions},
    shared::i18n,
};

mod sidebar;
mod toolbar;

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

thread_local! {
    static TAB_NODES: RefCell<HashMap<u64, (NodeRef, NodeRef)>> = RefCell::new(HashMap::new());
    static HEADING_NODES: RefCell<HashMap<&'static str, NodeRef>> = RefCell::new(HashMap::new());
    static LAST_VECTOR_INKS: RefCell<Vec<Rect<DevicePx, Device>>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn record_tab_nodes(id: u64, tab: NodeRef, corner: NodeRef) {
    TAB_NODES.with(|slot| {
        slot.borrow_mut().insert(id, (tab, corner));
    });
}

pub(super) fn record_heading_node(label: &'static str, node: NodeRef) {
    HEADING_NODES.with(|slot| {
        slot.borrow_mut().insert(label, node);
    });
}

pub(in crate::pages::workspace) fn heading_node(label: &'static str) -> Option<NodeRef> {
    HEADING_NODES.with(|slot| slot.borrow().get(label).copied())
}

pub(in crate::pages::workspace) fn recorded_tab_node(id: u64) -> Option<NodeRef> {
    TAB_NODES.with(|slot| slot.borrow().get(&id).map(|(tab, _)| *tab))
}

pub(in crate::pages::workspace) fn clear_recorded_tab_nodes() {
    TAB_NODES.with(|slot| slot.borrow_mut().clear());
    HEADING_NODES.with(|slot| slot.borrow_mut().clear());
    LAST_VECTOR_INKS.with(|slot| slot.borrow_mut().clear());
}

fn recorded_tab_nodes(id: u64) -> Option<(NodeRef, NodeRef)> {
    TAB_NODES.with(|slot| slot.borrow().get(&id).copied())
}

fn last_vector_inks() -> Vec<Rect<DevicePx, Device>> {
    LAST_VECTOR_INKS.with(|slot| slot.borrow().clone())
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

    fn draw(&mut self, scene: &zgui::scene::Scene, _: &zgui::bits::DamageSet) -> FrameOutcome {
        if !scene.primitives.mono_sprites.is_empty() {
            let placements = scene.placements();
            let matrices: Vec<Matrix4> = placements.matrices().collect();
            LAST_VECTOR_INKS.with(|slot| {
                let mut inks = slot.borrow_mut();
                inks.clear();
                for sprite in &scene.primitives.mono_sprites {
                    let [x, y, w, h] = sprite.bounds;
                    if w <= 0.0 || h <= 0.0 {
                        continue;
                    }
                    let local = Rect::new(
                        Point::new(DevicePx(x), DevicePx(y)),
                        Size::new(DevicePx(w), DevicePx(h)),
                    );
                    let placed = match matrices.get(sprite.transform as usize) {
                        Some(matrix) => transformed_bounds(matrix, local),
                        None => local,
                    };
                    inks.push(placed);
                }
            });
        }
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
struct LiveButton {
    label: String,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl LiveButton {
    fn center_x(&self) -> f64 {
        (self.x0 + self.x1) * 0.5
    }

    fn center_y(&self) -> f64 {
        (self.y0 + self.y1) * 0.5
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

fn collect_live_role(surface: &OffscreenSurface, role: Role) -> Vec<(LiveButton, bool)> {
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
        if node.role() == role
            && let Some(rect) = node.bounds()
        {
            let (x0, y0, x1, y1) = transform_rect(next_world, (rect.x0, rect.y0, rect.x1, rect.y1));
            out.push((
                LiveButton {
                    label: node.label().unwrap_or_default().to_owned(),
                    x0,
                    y0,
                    x1,
                    y1,
                },
                node.is_disabled(),
            ));
        }
        for &child in node.children().iter().rev() {
            stack.push((child, next_world));
        }
    }
    out
}

fn collect_live_buttons(surface: &OffscreenSurface) -> Vec<LiveButton> {
    collect_live_role(surface, Role::Button)
        .into_iter()
        .map(|(btn, _)| btn)
        .collect()
}

fn find_button_by_label<'a>(buttons: &'a [LiveButton], label: &str) -> Option<&'a LiveButton> {
    buttons.iter().find(|btn| btn.label == label)
}

fn click_at_with_modifiers(
    harness: &mut Harness<Box<dyn AppHandler>>,
    x: f32,
    y: f32,
    button: PointerButton,
    modifiers: Modifiers,
    ms: u64,
) {
    let mut ev = PointerEvent::mouse(Point::new(CssPx(x), CssPx(y)));
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Moved,
        event: ev,
        modifiers,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.pump();
    ev.button = Some(button);
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Pressed,
        event: ev,
        modifiers,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.pump();
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Released,
        event: ev,
        modifiers,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms + 50)),
    });
    harness.settle(16);
}

fn click_at(harness: &mut Harness<Box<dyn AppHandler>>, x: f32, y: f32, ms: u64) {
    click_at_with_modifiers(harness, x, y, PointerButton::Primary, Modifiers::NONE, ms);
}

pub(in crate::pages::workspace) fn click_button(
    harness: &mut Harness<Box<dyn AppHandler>>,
    label: &str,
) {
    let buttons = collect_live_buttons(&harness.platform().offscreens()[0]);
    let button =
        find_button_by_label(&buttons, label).unwrap_or_else(|| panic!("missing button {label:?}"));
    click_at(
        harness,
        button.center_x() as f32,
        button.center_y() as f32,
        100,
    );
}

fn run_chrome_harness(dir: &Path, check: impl FnOnce(&mut Harness<Box<dyn AppHandler>>, &Browser)) {
    run_browser_harness(
        dir,
        (1280.0, 800.0),
        volumes::Snapshot::default(),
        |browser| {
            let chrome = Chrome(browser);
            AnyView::new(view! {
                column(style:width = "100%", style:height = "100%") {
                    row(style:width = "100%", style:height = "38px") {
                        {chrome.toolbar_controls()}
                    }
                    row(style:flex = "1 1 0%", style:width = "100%", style:min-height = "0px") {
                        {chrome.sidebar(SidebarPresentation::default())}
                        {chrome.folder_filter()}
                    }
                }
            })
        },
        check,
    );
}

pub(in crate::pages::workspace) fn run_browser_harness(
    dir: &Path,
    size: (f32, f32),
    snapshot: volumes::Snapshot,
    render: impl Fn(&Browser) -> AnyView + 'static,
    check: impl FnOnce(&mut Harness<Box<dyn AppHandler>>, &Browser),
) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let scope = Scope::with_handle(runtime.handle().clone(), false);
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
            DevicePx(size.0),
            DevicePx(size.1),
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
        .with_size(size.0, size.1)
        .run_on(driver, move || {
            let scope = scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(i18n::prepare(Language::English).expect("English catalog"));
            let mut prefs = preferences::Preferences::default();
            prefs.files.sidebar.show_volumes = !snapshot.volumes.is_empty();
            let _prefs = preferences::Service::install(prefs, &scope).expect("preferences service");
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(Places::default()),
                    start: start.clone(),
                    home: start.clone(),
                    picker: None,
                },
                volumes::Service::with_snapshot(scope, snapshot.clone()),
                Rc::new(|_| {}),
            );
            *slot_for_view.borrow_mut() = Some(browser.clone());
            render(&browser)
        })
        .expect("headless chrome harness");
}
