use std::{
    cell::RefCell, collections::HashMap, collections::HashSet, path::Path, rc::Rc, sync::Arc,
    time::Duration,
};

use gds::{
    app::Gds,
    appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_ITEM_HEIGHT, SIDEBAR_WIDTH},
    i18n::Language,
    service::task_manager::{Scope, Supervisor},
    ui::sidebar::SidebarPresentation,
};
use zgui::{
    app::Fonts,
    atlas::{SinkError, TextureFormat, TextureId, TextureSink},
    geom::{CssPx, Device, DevicePx, Matrix4, Point, Rect, Size, transformed_bounds},
    platform::{AppHandler, PlatformError, Surface, SurfaceEvent},
    prelude::*,
    reactive::{LocalStorage, Mounted, Signal, flush, install},
    render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    },
    runtime::AppError,
    view,
    view::{ScrollBehavior, ScrollTarget},
    vocab::{Modifiers, PointerAction, PointerButton, PointerEvent, Role, Timestamp},
};
use zgui_platform_headless::{Harness, OffscreenSurface};

use super::{
    Breadcrumb, ROW_GAP, TabCycleTargets, compute_tab_cycle_targets, default_volumes_bottom_height,
    section_overflow_y,
};
use crate::{
    entities::{
        location::{
            Collection, Location, LocationTarget, places::Places, scheme::TagAddress, volumes,
        },
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

thread_local! {
    static TAB_NODES: RefCell<HashMap<u64, (NodeRef, NodeRef)>> = RefCell::new(HashMap::new());
    static HEADING_NODES: RefCell<HashMap<&'static str, NodeRef>> = RefCell::new(HashMap::new());
    static LOCATION_NODES: RefCell<Vec<(LocationTarget, NodeRef)>> = const { RefCell::new(Vec::new()) };
    static VOLUME_ICON_NODES: RefCell<Vec<(volumes::VolumeId, NodeRef)>> = const { RefCell::new(Vec::new()) };
    static LOCATIONS_PROGRESS: RefCell<Option<Signal<f32, LocalStorage>>> = const { RefCell::new(None) };
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

pub(super) fn record_location_node(target: LocationTarget, node: NodeRef) {
    LOCATION_NODES.with(|slot| {
        let mut items = slot.borrow_mut();
        if let Some((_, existing)) = items.iter_mut().find(|(t, _)| *t == target) {
            *existing = node;
        } else {
            items.push((target, node));
        }
    });
}

pub(super) fn record_volume_icon_node(id: volumes::VolumeId, node: NodeRef) {
    VOLUME_ICON_NODES.with(|slot| {
        let mut items = slot.borrow_mut();
        if let Some((_, existing)) = items.iter_mut().find(|(v, _)| *v == id) {
            *existing = node;
        } else {
            items.push((id, node));
        }
    });
}

pub(super) fn record_locations_progress(progress: Signal<f32, LocalStorage>) {
    LOCATIONS_PROGRESS.with(|slot| {
        *slot.borrow_mut() = Some(progress);
    });
}

pub(in crate::pages::workspace) fn clear_recorded_tab_nodes() {
    TAB_NODES.with(|slot| slot.borrow_mut().clear());
    HEADING_NODES.with(|slot| slot.borrow_mut().clear());
    LOCATION_NODES.with(|slot| slot.borrow_mut().clear());
    VOLUME_ICON_NODES.with(|slot| slot.borrow_mut().clear());
    LOCATIONS_PROGRESS.with(|slot| *slot.borrow_mut() = None);
    LAST_VECTOR_INKS.with(|slot| slot.borrow_mut().clear());
}

fn recorded_tab_nodes(id: u64) -> Option<(NodeRef, NodeRef)> {
    TAB_NODES.with(|slot| slot.borrow().get(&id).copied())
}

fn recorded_location_nodes() -> Vec<(LocationTarget, NodeRef)> {
    LOCATION_NODES.with(|slot| slot.borrow().clone())
}

fn recorded_volume_icon_node(id: &volumes::VolumeId) -> Option<NodeRef> {
    VOLUME_ICON_NODES.with(|slot| {
        slot.borrow()
            .iter()
            .find(|(vid, _)| vid == id)
            .map(|(_, node)| *node)
    })
}

fn recorded_locations_progress() -> Option<Signal<f32, LocalStorage>> {
    LOCATIONS_PROGRESS.with(|slot| *slot.borrow())
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

    fn height(&self) -> f64 {
        self.y1 - self.y0
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

fn collect_live_buttons(surface: &OffscreenSurface) -> Vec<LiveButton> {
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
        if node.role() == Role::Button
            && let Some(rect) = node.bounds()
        {
            let (x0, y0, x1, y1) = transform_rect(next_world, (rect.x0, rect.y0, rect.x1, rect.y1));
            out.push(LiveButton {
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

fn find_button_by_label<'a>(buttons: &'a [LiveButton], label: &str) -> Option<&'a LiveButton> {
    buttons.iter().find(|btn| btn.label == label)
}

fn run_chrome_harness(dir: &Path, check: impl FnOnce(&mut Harness<Box<dyn AppHandler>>, &Browser)) {
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
            let _ = app.appearance().install_window_sheet();
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
            view! {
                column(style:width = "100%", style:height = "100%") {
                    row(style:width = "100%", style:height = "38px") {
                        {browser.toolbar_controls()}
                    }
                    row(style:flex = "1 1 0%", style:width = "100%", style:min-height = "0px") {
                        {browser.sidebar(SidebarPresentation::default())}
                        {browser.folder_filter()}
                    }
                }
            }
        })
        .expect("headless chrome harness");
}

#[test]
fn filesystem_breadcrumbs_ignore_curdir_and_mark_only_leaf_as_current() {
    let crumbs = Breadcrumb::for_location(
        &Location::from(Path::new("/mnt/./storage/projects")),
        Some(Path::new("/mnt/./storage/projects")),
    );
    assert_eq!(crumbs.len(), 4);
    assert_eq!(crumbs[0].label, "/");
    assert!(!crumbs[0].current);
    assert!(!crumbs[0].separated);
    assert_eq!(crumbs[1].label, "mnt/");
    assert_eq!(crumbs[1].location, Location::from(Path::new("/mnt")));
    assert_eq!(crumbs[2].label, "storage/");
    assert_eq!(crumbs[3].label, "projects");
    assert!(crumbs[3].current);
}

#[test]
fn tag_breadcrumbs_build_clickable_separated_hierarchy_from_root_to_leaf() {
    let _ = install();
    let root = Mounted::new();
    root.with(|| {
        let app = Gds::install(Scope::default());
        app.i18n()
            .install_catalog(i18n::prepare(Language::English).expect("English catalog"));

        let leaf = TagAddress::new("work/urgent").expect("valid tag path");
        let crumbs = Breadcrumb::for_location(&Location::Tags(leaf.clone()), None);
        let root_tag = TagAddress::new("").expect("root tag");
        let work_tag = TagAddress::new("work").expect("work tag");

        assert_eq!(crumbs.len(), 3);
        assert_eq!(crumbs[0].location, Location::Tags(root_tag));
        assert!(!crumbs[0].current);
        assert!(!crumbs[0].separated);

        assert_eq!(crumbs[1].label, "work");
        assert_eq!(crumbs[1].location, Location::Tags(work_tag));
        assert!(!crumbs[1].current);
        assert!(crumbs[1].separated);

        assert_eq!(crumbs[2].label, "urgent");
        assert_eq!(crumbs[2].location, Location::Tags(leaf));
        assert!(crumbs[2].current);
        assert!(crumbs[2].separated);
    });
    root.unmount();
}

#[test]
fn tab_cycle_targets_wrap_around_strip_and_stay_empty_for_single_tab() {
    assert_eq!(
        compute_tab_cycle_targets(&[7], 7),
        TabCycleTargets::default()
    );
    assert_eq!(
        compute_tab_cycle_targets(&[10, 20, 30], 10),
        TabCycleTargets {
            previous: Some(30),
            next: Some(20),
        }
    );
    assert_eq!(
        compute_tab_cycle_targets(&[10, 20], 10),
        TabCycleTargets {
            previous: Some(20),
            next: Some(20),
        }
    );
}

#[test]
fn volumes_default_bottom_height_caps_visible_and_mounted_rows_at_four() {
    let single_unmounted = default_volumes_bottom_height(1, 0);
    let single_mounted = default_volumes_bottom_height(1, 1);
    let four_mounted = default_volumes_bottom_height(4, 4);
    let eight_mounted = default_volumes_bottom_height(8, 8);

    assert!(single_mounted > single_unmounted);
    assert_eq!(four_mounted, eight_mounted);
}

#[test]
fn responsive_toolbar_and_collapsed_rail_center_actions_on_29px_axis() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let temp = tempfile::tempdir().expect("tempdir");

    let pending_label = temp.path().display().to_string();

    run_chrome_harness(temp.path(), |harness, browser| {
        let surface = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("offscreen surface");

        let desktop_buttons = collect_live_buttons(&surface);
        let toggle_lbl = gds::i18n::text_untracked("sidebar-toggle");
        let back_lbl = gds::i18n::text_untracked("back");
        let forward_lbl = gds::i18n::text_untracked("forward");
        let parent_lbl = gds::i18n::text_untracked("parent");
        let new_tab_lbl = gds::i18n::text_untracked("new-tab");

        assert!(find_button_by_label(&desktop_buttons, &pending_label).is_some());
        assert!(find_button_by_label(&desktop_buttons, &toggle_lbl).is_some());
        assert!(find_button_by_label(&desktop_buttons, &back_lbl).is_some());
        assert!(find_button_by_label(&desktop_buttons, &forward_lbl).is_some());
        assert!(find_button_by_label(&desktop_buttons, &parent_lbl).is_some());
        assert_eq!(browser.sidebar_width().get_untracked(), SIDEBAR_WIDTH);

        browser.update(|ws| ws.tab_mut().navigation.cancel_pending());
        browser.0.window_width.set(960.0);
        flush();
        harness.settle(16);

        let compact_buttons = collect_live_buttons(&surface);
        assert!(find_button_by_label(&compact_buttons, &toggle_lbl).is_none());
        assert!(find_button_by_label(&compact_buttons, &back_lbl).is_none());
        assert!(find_button_by_label(&compact_buttons, &forward_lbl).is_none());
        assert!(find_button_by_label(&compact_buttons, &parent_lbl).is_none());
        assert!(find_button_by_label(&compact_buttons, "/").is_some());
        assert_eq!(
            browser.sidebar_width().get_untracked(),
            COLLAPSED_SIDEBAR_WIDTH
        );

        let new_tab_btn = find_button_by_label(&compact_buttons, &new_tab_lbl)
            .expect("new-tab button in collapsed rail");
        let locations_lbl = gds::i18n::text_untracked("locations");
        let locations_btn = find_button_by_label(&compact_buttons, &locations_lbl)
            .expect("locations heading button in collapsed rail");
        let rail_center = f64::from(COLLAPSED_SIDEBAR_WIDTH) * 0.5;
        assert!(
            (new_tab_btn.center_x() - rail_center).abs() <= 1.0,
            "expected collapsed + button centered at x={rail_center}, got {:.2} ({:.1}..{:.1})",
            new_tab_btn.center_x(),
            new_tab_btn.x0,
            new_tab_btn.x1
        );
        assert!(
            (locations_btn.center_x() - rail_center).abs() <= 1.0
                && locations_btn.x1 <= f64::from(COLLAPSED_SIDEBAR_WIDTH),
            "expected collapsed locations heading centered at x={rail_center}, got {:.2} ({:.1}..{:.1})",
            locations_btn.center_x(),
            locations_btn.x0,
            locations_btn.x1
        );
    });
}

#[test]
fn collapsed_pinned_tab_renders_single_corner_pin_button_without_pinned_icon_badge() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let temp = tempfile::tempdir().expect("tempdir");
    let sub = temp.path().join("workspace_dir");
    std::fs::create_dir_all(&sub).expect("create workspace_dir");
    clear_recorded_tab_nodes();

    run_chrome_harness(&sub, |harness, browser| {
        let surface = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("offscreen surface");

        browser.update(|ws| ws.tab_mut().navigation.cancel_pending());
        browser.0.window_width.set(960.0);
        flush();
        harness.settle(16);

        let tab_id = browser.read_untracked(|ws| ws.tab().id);
        let (unpinned_tab_ref, _) =
            recorded_tab_nodes(tab_id).expect("recorded unpinned tab nodes");
        let unpinned_tab_bounds = unpinned_tab_ref
            .window_bounds()
            .expect("unpinned tab bounds");
        let unpinned_vectors: Vec<_> = last_vector_inks()
            .into_iter()
            .filter(|ink| {
                ink.origin.y.0 >= unpinned_tab_bounds.origin.y.0 - 2.0
                    && (ink.origin.y.0 + ink.size.height.0)
                        <= (unpinned_tab_bounds.origin.y.0
                            + unpinned_tab_bounds.size.height.0
                            + 2.0)
            })
            .collect();
        assert!(
            !unpinned_vectors.is_empty(),
            "expected folder icon vectors on collapsed unpinned tab"
        );

        // Pin the tab while sidebar is collapsed (58px rail).
        browser.toggle_pin(0);
        flush();
        harness.settle(16);

        let (pinned_tab_ref, corner_ref) =
            recorded_tab_nodes(tab_id).expect("recorded pinned tab nodes");
        let tab_bounds = pinned_tab_ref.window_bounds().expect("pinned tab bounds");
        let corner_bounds = corner_ref
            .window_bounds()
            .expect("corner pin button bounds");

        let unpin_lbl = gds::i18n::text_untracked("unpin-tab");
        let unpin_buttons: Vec<_> = collect_live_buttons(&surface)
            .into_iter()
            .filter(|btn| btn.label == unpin_lbl)
            .collect();
        assert_eq!(
            unpin_buttons.len(),
            1,
            "collapsed pinned tab must expose only the single CornerButton unpin action, got {unpin_buttons:?}"
        );
        let rail_center = f64::from(COLLAPSED_SIDEBAR_WIDTH) * 0.5;
        assert!(
            unpin_buttons[0].center_x() > rail_center,
            "expected CornerButton pin icon in top-right of collapsed rail (> {rail_center}), got {:?}",
            unpin_buttons[0]
        );

        let pinned_vectors: Vec<_> = last_vector_inks()
            .into_iter()
            .filter(|ink| {
                ink.origin.y.0 >= tab_bounds.origin.y.0 - 4.0
                    && (ink.origin.y.0 + ink.size.height.0)
                        <= (tab_bounds.origin.y.0 + tab_bounds.size.height.0 + 4.0)
            })
            .collect();

        // Each visible `<vector>` Icon rasterizes to one `ColorSprite`:
        // - Unpinned collapsed tab: 1 icon sprite (centered 18x18 folder icon).
        // - Pinned collapsed tab: 2 icon sprites (centered 18x18 folder icon + single 11x11
        //   `CornerButton` Pin icon inside `corner_bounds`), NEVER 3 from a duplicate `PinnedIcon`!
        assert_eq!(
            unpinned_vectors.len(),
            1,
            "expected exactly 1 icon sprite on collapsed unpinned tab, got {unpinned_vectors:?}"
        );
        assert_eq!(
            pinned_vectors.len(),
            2,
            "expected exactly 2 icon sprites on collapsed pinned tab (folder + single CornerButton pin, never duplicate PinnedIcon), got {pinned_vectors:?}"
        );
        let has_top_left_pin_badge = pinned_vectors.iter().any(|ink| {
            let cx = ink.origin.x.0 + ink.size.width.0 * 0.5;
            let in_corner_btn = ink.origin.x.0 >= corner_bounds.origin.x.0 - 2.0
                && (ink.origin.x.0 + ink.size.width.0)
                    <= corner_bounds.origin.x.0 + corner_bounds.size.width.0 + 2.0;
            !in_corner_btn && f64::from(cx) < rail_center - 4.0
        });
        assert!(
            !has_top_left_pin_badge,
            "collapsed pinned tab must not render a top-left PinnedIcon badge; vectors={pinned_vectors:?}"
        );
    });
}

#[test]
fn locations_collapse_mid_transition_hides_scrollbar_and_keeps_full_38px_row_heights() {
    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");
    clear_recorded_tab_nodes();

    let start = home;
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        let loc_progress = recorded_locations_progress().expect("recorded loc_progress signal");
        let locations_node = browser.0.locations_node;
        let scale = locations_node.scale().max(1.0);

        let tags_enabled = browser
            .0
            .preferences
            .state()
            .with_untracked(|p| p.tags.enabled);
        let expected_count = LocationTarget::ALL
            .iter()
            .filter(|t| tags_enabled || !matches!(t, LocationTarget::Collection(Collection::Tags)))
            .count();
        let full = (expected_count as f32 * (SIDEBAR_ITEM_HEIGHT + ROW_GAP) - ROW_GAP).max(0.0);

        // Initially settled open (`loc_progress == 1.0`): full height and `overflow-y == auto`.
        assert!(
            (loc_progress.get_untracked() - 1.0).abs() < 1e-4,
            "expected initial loc_progress == 1.0, got {}",
            loc_progress.get_untracked()
        );
        assert_eq!(section_overflow_y(loc_progress.get_untracked()), "auto");
        let open_bounds = locations_node
            .bounds()
            .expect("settled open .gds-files-sidebar-locations bounds");
        let open_h = open_bounds.size.height.0 / scale;
        assert!(
            (open_h - full).abs() < 0.5,
            "expected settled open .gds-files-sidebar-locations height == {full:.1}px, got {open_h:.1}px"
        );

        // Toggle `Locations` collapse and sample mid-transition (`0.15 < loc_progress < 0.85`).
        browser.edit_preferences(|p| {
            p.files.sidebar.locations_collapsed = true;
        });
        flush();
        harness.run_for(Duration::from_millis(48), Duration::from_millis(16));
        flush();
        harness.settle(2);

        let mid_progress = loc_progress.get_untracked();
        assert!(
            mid_progress > 0.15 && mid_progress < 0.85,
            "expected mid-transition loc_progress in (0.15, 0.85), got {mid_progress:.3}"
        );
        assert_eq!(
            section_overflow_y(mid_progress),
            "hidden",
            "expected overflow-y == hidden while loc_progress < 0.999"
        );

        // 1) `.gds-files-sidebar-locations` has `0 < height < full` and `overflow-y == hidden`
        //    (verified both on `section_overflow_y` and by confirming engine scroll clamping!).
        let mid_bounds = locations_node
            .bounds()
            .expect("mid-transition .gds-files-sidebar-locations bounds");
        let mid_h = mid_bounds.size.height.0 / scale;
        assert!(
            mid_h > 0.0 && mid_h < full - 10.0,
            "expected 0 < mid height < {full:.1}px during collapse, got {mid_h:.1}px (progress={mid_progress:.3})"
        );
        locations_node.scroll_to(
            ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(40.0))),
            ScrollBehavior::Instant,
        );
        harness.settle(2);
        assert!(
            locations_node.scroll_offset().y.0.abs() < 0.1,
            "expected .gds-files-sidebar-locations with overflow-y: hidden to disallow vertical scrolling mid-transition, got offset {:?}",
            locations_node.scroll_offset()
        );

        // 2) Every `.gds-files-location-item` inside it still has its full 38px layout height
        //    (`>= 37.5px`, ZERO vertical pancake squishing!).
        let items = recorded_location_nodes();
        assert_eq!(
            items.len(),
            expected_count,
            "expected {expected_count} recorded .gds-files-location-item nodes"
        );
        for (target, item_ref) in items {
            let item_bounds = item_ref
                .bounds()
                .unwrap_or_else(|| panic!("measured bounds for {target:?} mid-collapse"));
            let item_h = item_bounds.size.height.0 / scale;
            assert!(
                item_h >= 37.5 && (item_h - SIDEBAR_ITEM_HEIGHT).abs() < 0.5,
                "expected {target:?} .gds-files-location-item height == 38.0px (>= 37.5px, zero vertical squishing) mid-collapse at height={mid_h:.1}px, got {item_h:.2}px"
            );
        }

        // Complete collapse, then expand back and verify mid-expand + settled-open invariants.
        harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
        flush();
        harness.settle(4);
        assert!(
            loc_progress.get_untracked() <= 1e-4 && locations_node.bounds().is_none(),
            "expected collapsed Locations section to hide from layout at loc_progress == 0"
        );

        browser.edit_preferences(|p| {
            p.files.sidebar.locations_collapsed = false;
        });
        flush();
        harness.run_for(Duration::from_millis(48), Duration::from_millis(16));
        flush();
        harness.settle(2);
        let expand_mid = loc_progress.get_untracked();
        assert!(
            expand_mid > 0.15 && expand_mid < 0.85,
            "expected mid-expand loc_progress in (0.15, 0.85), got {expand_mid:.3}"
        );
        assert_eq!(section_overflow_y(expand_mid), "hidden");
        for (target, item_ref) in recorded_location_nodes() {
            let item_h = item_ref
                .bounds()
                .unwrap_or_else(|| panic!("measured bounds for {target:?} mid-expand"))
                .size
                .height
                .0
                / scale;
            assert!(
                item_h >= 37.5 && (item_h - SIDEBAR_ITEM_HEIGHT).abs() < 0.5,
                "expected {target:?} height == 38.0px mid-expand, got {item_h:.2}px"
            );
        }

        harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
        flush();
        harness.settle(4);
        assert_eq!(section_overflow_y(loc_progress.get_untracked()), "auto");

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(true);
            let typo = gds::appearance::typography::Preferences {
                size: 16.0,
                ..Default::default()
            };
            app.appearance()
                .typography()
                .apply(&typo)
                .expect("16px typography");
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(i18n::prepare(Language::Chinese).expect("Chinese catalog"));
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
            view! {
                column(style:width = "100%", style:height = "100%") {
                    row(style:width = "100%", style:height = "38px") {
                        {browser.toolbar_controls()}
                    }
                    row(style:flex = "1 1 0%", style:width = "100%", style:min-height = "0px") {
                        {browser.sidebar(SidebarPresentation::default())}
                        {browser.folder_filter()}
                    }
                }
            }
        })
        .expect("headless locations collapse transition harness");
}

fn has_live_tooltip(surface: &OffscreenSurface) -> bool {
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
        return false;
    };
    let mut stack = vec![root_id];
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(&id) else {
            continue;
        };
        if node.is_hidden() {
            continue;
        }
        if node.role() == Role::Tooltip {
            return true;
        }
        for &child in node.children().iter().rev() {
            stack.push(child);
        }
    }
    false
}

#[test]
fn sidebar_heading_keeps_single_dom_node_and_shows_tooltip_only_when_rail_is_collapsed() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let temp = tempfile::tempdir().expect("tempdir");
    clear_recorded_tab_nodes();

    run_chrome_harness(temp.path(), |harness, browser| {
        let surface = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("offscreen surface");

        let heading = heading_node("locations").expect("recorded locations sidebar heading");
        let scale = heading.scale().max(1.0);
        let initial_id = heading.get_untracked().expect("bound heading NodeId");

        // 1. Hovering heading while expanded (`220px`) must NOT open a tooltip.
        let exp_bounds = heading.window_bounds().expect("expanded heading bounds");
        let exp_pos = Point::new(
            CssPx((exp_bounds.origin.x.0 + 16.0) / scale),
            CssPx((exp_bounds.origin.y.0 + exp_bounds.size.height.0 * 0.5) / scale),
        );
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(exp_pos),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        harness.advance(Duration::from_millis(500));
        harness.settle(8);
        assert!(
            !has_live_tooltip(&surface),
            "sidebar_heading must suppress its tooltip while the sidebar is expanded"
        );

        // Move pointer away and click `PanelLeft` toolbar button to collapse sidebar.
        let toggle_lbl = gds::i18n::text_untracked("sidebar-toggle");
        let live_buttons = collect_live_buttons(&surface);
        let toggle_btn = find_button_by_label(&live_buttons, &toggle_lbl)
            .expect("PanelLeft sidebar-toggle button");
        let toggle_pos = Point::new(CssPx(toggle_btn.center_x() as f32), CssPx(19.0));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(toggle_pos),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(toggle_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(toggle_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(16);

        assert!(
            !browser.0.sidebar.get_untracked(),
            "clicking PanelLeft must collapse sidebar"
        );
        assert_eq!(
            browser.sidebar_width().get_untracked(),
            COLLAPSED_SIDEBAR_WIDTH
        );
        assert_eq!(
            heading.get_untracked(),
            Some(initial_id),
            "sidebar_heading must keep the exact same NodeId when collapsing"
        );

        // 2. Hovering heading while collapsed (`58px` rail) MUST reveal its tooltip after 400ms.
        let col_bounds = heading.window_bounds().expect("collapsed heading bounds");
        let col_pos = Point::new(
            CssPx((col_bounds.origin.x.0 + col_bounds.size.width.0 * 0.5) / scale),
            CssPx((col_bounds.origin.y.0 + col_bounds.size.height.0 * 0.5) / scale),
        );
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(col_pos),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        harness.advance(Duration::from_millis(500));
        harness.settle(8);
        assert!(
            has_live_tooltip(&surface),
            "sidebar_heading must reveal its tooltip when hovered on the collapsed rail"
        );

        // 3. Clicking `PanelLeft` a second time expands the sidebar and closes the heading tooltip.
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(toggle_pos),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(toggle_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(toggle_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(Point::new(CssPx(640.0), CssPx(400.0))),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(16);

        assert!(
            browser.0.sidebar.get_untracked(),
            "second click on PanelLeft must expand sidebar"
        );
        assert_eq!(browser.sidebar_width().get_untracked(), SIDEBAR_WIDTH);
        harness.advance(Duration::from_millis(250));
        harness.settle(8);
        assert!(
            !has_live_tooltip(&surface),
            "expanding sidebar must close collapsed-rail heading tooltip"
        );
        assert_eq!(
            heading.get_untracked(),
            Some(initial_id),
            "sidebar_heading must keep the exact same NodeId when expanding"
        );
    });
}

#[test]
fn sidebar_volume_rows_eliminate_duplicate_insets_and_center_icons_at_29px_when_collapsed() {
    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let mounted_dir = temp.path().join("mnt-usb");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&mounted_dir).expect("create mnt-usb");
    clear_recorded_tab_nodes();

    let mounted_id = volumes::VolumeId {
        stable: Some("UUID-MOUNTED".to_owned()),
        attachment: "usb-1".to_owned(),
    };
    let unmounted_id = volumes::VolumeId {
        stable: Some("UUID-UNMOUNTED".to_owned()),
        attachment: "usb-2".to_owned(),
    };
    let snapshot = volumes::Snapshot {
        drives: vec![volumes::Drive {
            id: "drive-usb".to_owned(),
            name: "Flash Drive".to_owned(),
            kind: volumes::DeviceKind::Usb,
            can_power_off: true,
            can_eject: false,
            sibling_id: None,
        }],
        volumes: vec![
            volumes::Volume {
                id: mounted_id.clone(),
                object: "/org/freedesktop/UDisks2/block_devices/sdb1".to_owned(),
                drive: Some("drive-usb".to_owned()),
                name: "Portable".to_owned(),
                size: 64 * 1024 * 1024 * 1024,
                label: "Portable".to_owned(),
                filesystem: "ext4".to_owned(),
                read_only: false,
                locked: false,
                mountable: true,
                system_mount: false,
                mount_points: vec![mounted_dir],
                visible: true,
            },
            volumes::Volume {
                id: unmounted_id.clone(),
                object: "/org/freedesktop/UDisks2/block_devices/sdb2".to_owned(),
                drive: Some("drive-usb".to_owned()),
                name: "Archive".to_owned(),
                size: 128 * 1024 * 1024 * 1024,
                label: "Archive".to_owned(),
                filesystem: "btrfs".to_owned(),
                read_only: false,
                locked: false,
                mountable: true,
                system_mount: false,
                mount_points: Vec::new(),
                visible: true,
            },
        ],
    };

    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let start = home;

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        let mounted_icon =
            recorded_volume_icon_node(&mounted_id).expect("mounted volume icon node");
        let unmounted_icon =
            recorded_volume_icon_node(&unmounted_id).expect("unmounted volume icon node");
        let scale = mounted_icon.scale().max(1.0);
        let mounted_node_id = mounted_icon.get_untracked().expect("mounted icon NodeId");
        let unmounted_node_id = unmounted_icon
            .get_untracked()
            .expect("unmounted icon NodeId");

        // Expanded (222px): 10px section padding + 8px row pad-left = 18px left edge (center 27px),
        // matching tabs and locations without any duplicate 26px volume-row inset.
        let exp_mounted = mounted_icon.window_bounds().expect("expanded mounted icon");
        let exp_unmounted = unmounted_icon
            .window_bounds()
            .expect("expanded unmounted icon");
        assert!(
            (exp_mounted.origin.x.0 / scale - 18.0).abs() <= 0.5,
            "expected expanded mounted volume icon x == 18.0px (never 26.0px), got {:.2}",
            exp_mounted.origin.x.0 / scale
        );
        assert!(
            (exp_unmounted.origin.x.0 / scale - 18.0).abs() <= 0.5,
            "expected expanded unmounted volume icon x == 18.0px (never 26.0px), got {:.2}",
            exp_unmounted.origin.x.0 / scale
        );

        // Collapse sidebar and verify mid-transition (48ms) smooth interpolation + settled 29px rail center.
        browser.toggle_sidebar();
        flush();
        harness.run_for(Duration::from_millis(48), Duration::from_millis(16));
        flush();
        harness.settle(2);
        let mid_bounds = mounted_icon
            .window_bounds()
            .expect("mid-collapse mounted icon");
        let mid_x = mid_bounds.origin.x.0 / scale;
        assert!(
            mid_x > 18.05 && mid_x < 19.95,
            "expected volume icon x to interpolate smoothly between 18px and 20px mid-collapse, got {mid_x:.2}"
        );

        harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
        flush();
        harness.settle(4);
        let rail_center = COLLAPSED_SIDEBAR_WIDTH * 0.5;
        let col_mounted = mounted_icon
            .window_bounds()
            .expect("collapsed mounted icon");
        let col_unmounted = unmounted_icon
            .window_bounds()
            .expect("collapsed unmounted icon");
        let col_mounted_cx = (col_mounted.origin.x.0 + col_mounted.size.width.0 * 0.5) / scale;
        let col_unmounted_cx =
            (col_unmounted.origin.x.0 + col_unmounted.size.width.0 * 0.5) / scale;
        assert!(
            (col_mounted_cx - rail_center).abs() <= 0.5,
            "expected collapsed mounted volume icon centered at x={rail_center} (never 39px), got {col_mounted_cx:.2}"
        );
        assert!(
            (col_unmounted_cx - rail_center).abs() <= 0.5,
            "expected collapsed unmounted volume icon centered at x={rail_center} (never 39px), got {col_unmounted_cx:.2}"
        );
        assert_eq!(mounted_icon.get_untracked(), Some(mounted_node_id));
        assert_eq!(unmounted_icon.get_untracked(), Some(unmounted_node_id));

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(true);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(i18n::prepare(Language::English).expect("English catalog"));
            let mut initial_prefs = preferences::Preferences::default();
            initial_prefs.files.sidebar.show_volumes = true;
            initial_prefs.files.sidebar.volumes_collapsed = false;
            let _prefs =
                preferences::Service::install(initial_prefs, &scope).expect("preferences service");
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
            view! {
                row(style:width = "100%", style:height = "100%") {
                    {browser.sidebar(SidebarPresentation::default())}
                }
            }
        })
        .expect("headless volume sidebar inset harness");
}

#[test]
fn overflowing_address_breadcrumbs_stay_vertically_centered_with_edit_button() {
    let supervisor = Supervisor::new(false).expect("test runtime");
    let bg_scope = supervisor.scope();
    let temp = tempfile::tempdir().expect("tempdir");
    let deep = temp
        .path()
        .join("gds-vm-e2e/run-20261121T114232-573306Z/ubuntu/F19_terminal_writes_in_current_directory/work");
    std::fs::create_dir_all(&deep).expect("create deep directory");

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
            DevicePx(480.0),
            DevicePx(120.0),
        )));
        harness.settle(8);

        let buttons = collect_live_buttons(&offscreen);
        let root = find_button_by_label(&buttons, "/").expect("root breadcrumb button");
        let leaf = find_button_by_label(&buttons, "work").expect("leaf breadcrumb button");
        let edit = find_button_by_label(&buttons, "Edit location").expect("edit location button");

        assert!(
            leaf.x1 - leaf.x0 > 12.0,
            "expected leaf breadcrumb text to have measured width > 12px, got {:.2} (leaf={leaf:?})",
            leaf.x1 - leaf.x0
        );
        assert!(
            root.x0 < 0.0,
            "expected deep path to overflow horizontally and scroll root crumb offscreen left, got root={root:?}, leaf={leaf:?}, edit={edit:?}"
        );

        let leaf_height = leaf.height();
        let leaf_cy = leaf.center_y();
        let edit_cy = edit.center_y();
        assert!(
            (leaf_height - 24.0).abs() <= 1.0,
            "expected leaf breadcrumb height ~24px (no bottom scrollbar gutter squeezing track), got {leaf_height:.2} (leaf={leaf:?}, edit={edit:?})"
        );
        assert!(
            (leaf_cy - edit_cy).abs() <= 1.0,
            "expected overflowing breadcrumb center y={leaf_cy:.2} to align with edit button center y={edit_cy:.2} (leaf={leaf:?}, edit={edit:?})"
        );
        assert!(
            leaf.x1 <= edit.x0 + 1.0 && leaf.x0 >= 0.0,
            "expected overflowing breadcrumb track to scroll right so leaf ({:.1}..{:.1}) stays visible before edit button ({:.1}..{:.1}); all buttons: {buttons:?}",
            leaf.x0,
            leaf.x1,
            edit.x0,
            edit.x1
        );

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::system())
        .with_renderer(Box::new(noop_renderer))
        .with_size(480.0, 120.0)
        .run_on(driver, move || {
            let scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(i18n::prepare(Language::English).expect("English catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
                .expect("preferences service");
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(Places::default()),
                    start: deep.clone(),
                    home: deep.clone(),
                    picker: None,
                },
                volumes::Service::new(scope),
                Rc::new(|_| {}),
            );
            view! {
                row(style:width = "100%", style:height = "48px", style:align_items = "center") {
                    {browser.address_bar()}
                }
            }
        })
        .expect("headless overflowing breadcrumb harness");
}
