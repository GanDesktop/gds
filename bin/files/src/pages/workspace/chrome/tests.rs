use std::{
    cell::RefCell, collections::HashMap, collections::HashSet, path::Path, rc::Rc, sync::Arc,
    time::Duration,
};

use gds::{
    app::Gds,
    appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH},
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
    reactive::{Mounted, flush, install},
    render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    },
    runtime::AppError,
    view,
    vocab::{
        KeyCode, KeyEvent, Modifiers, NamedKey, PhysicalKey, PointerAction, PointerButton,
        PointerEvent, Role, Timestamp,
    },
};
use zgui_platform_headless::{Harness, OffscreenSurface};

use super::{Breadcrumb, TabCycleTargets, compute_tab_cycle_targets};
use crate::pages::workspace::view::tests::send_key_chord;
use crate::{
    entities::{
        location::{Location, places::Places, scheme::TagAddress, volumes},
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

fn click_at(harness: &mut Harness<Box<dyn AppHandler>>, x: f32, y: f32, ms: u64) {
    let mut ev = PointerEvent::mouse(Point::new(CssPx(x), CssPx(y)));
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Moved,
        event: ev,
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.settle(4);
    ev.button = Some(PointerButton::Primary);
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Pressed,
        event: ev,
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.settle(4);
    harness.deliver_to_first(SurfaceEvent::Pointer {
        action: PointerAction::Released,
        event: ev,
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms + 50)),
    });
    harness.settle(8);
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
            AnyView::new(view! {
                column(style:width = "100%", style:height = "100%") {
                    row(style:width = "100%", style:height = "38px") {
                        {browser.toolbar_controls()}
                    }
                    row(style:flex = "1 1 0%", style:width = "100%", style:min-height = "0px") {
                        {browser.sidebar(SidebarPresentation::default())}
                        {browser.folder_filter()}
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
fn mounted_device_below_the_fold_can_be_opened_from_the_keyboard() {
    let temp = tempfile::tempdir().expect("temporary mount roots");
    let names = ["Backup", "Camera", "Music", "Photos", "Archive"];
    let mounted = |name: &str| {
        let mount = temp.path().join(name);
        std::fs::create_dir(&mount).expect("create mount directory");
        volumes::Volume {
            id: volumes::VolumeId {
                stable: Some(name.to_owned()),
                attachment: name.to_owned(),
            },
            object: name.to_owned(),
            drive: None,
            name: name.to_owned(),
            size: 0,
            label: name.to_owned(),
            filesystem: "ext4".into(),
            read_only: false,
            locked: false,
            mountable: true,
            system_mount: false,
            mount_points: vec![mount],
            visible: true,
        }
    };
    let snapshot = volumes::Snapshot {
        volumes: names.map(mounted).into(),
        ..Default::default()
    };
    run_browser_harness(
        temp.path(),
        (1280.0, 320.0),
        snapshot,
        |browser| AnyView::new(browser.view()),
        |harness, browser| {
            let surface = harness.platform().offscreens()[0].clone();
            let viewport = browser
                .0
                .volumes_node
                .window_bounds()
                .expect("volume list viewport");
            let buttons = collect_live_buttons(&surface);
            let target = find_button_by_label(&buttons, "Archive").expect("fifth device");
            let bottom = f64::from(viewport.origin.y.0 + viewport.size.height.0);
            assert!(
                target.center_y() >= bottom,
                "fifth device should start below the visible portion of the list"
            );
            browser.focus_content();
            harness.settle(8);
            send_key_chord(harness, KeyEvent::character("v"), Modifiers::ALT);
            send_key_chord(
                harness,
                KeyEvent::named(NamedKey::End, PhysicalKey::Code(KeyCode::End)),
                Modifiers::NONE,
            );
            send_key_chord(
                harness,
                KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
                Modifiers::NONE,
            );
            let destination = temp.path().join("Archive");
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while browser.read_untracked(|ws| ws.tab().navigation.path() != Some(&destination)) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out opening the fifth mounted device"
                );
                harness.advance(Duration::from_millis(16));
                harness.pump();
            }
        },
    );
}

#[test]
fn new_tab_can_be_opened_from_the_collapsed_sidebar() {
    let temp = tempfile::tempdir().expect("tempdir");
    run_chrome_harness(temp.path(), |harness, browser| {
        browser.0.window_width.set(960.0);
        flush();
        harness.settle(16);
        let original = browser.read_untracked(|ws| ws.tab().id);
        click_button(harness, &gds::i18n::text_untracked("new-tab"));
        assert_eq!(browser.read_untracked(|ws| ws.tabs.len()), 2);
        assert_ne!(browser.read_untracked(|ws| ws.tab().id), original);
    });
}

#[test]
fn collapsed_pinned_tab_renders_single_corner_pin_button_without_pinned_icon_badge() {
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
fn overflowing_address_bar_keeps_the_leaf_and_edit_button_visible() {
    let supervisor = Supervisor::new(false).expect("test runtime");
    let bg_scope = supervisor.scope();
    let temp = tempfile::tempdir().expect("tempdir");
    let deep = temp.path().join(
        "projects/desktop/integration/fixtures/documents/quarterly-reports/working-drafts/work",
    );
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
            root.x0 < 0.0,
            "expected deep path to overflow horizontally and scroll root crumb offscreen left, got root={root:?}, leaf={leaf:?}, edit={edit:?}"
        );

        assert!(
            leaf.x0 >= 0.0 && leaf.x0 < leaf.x1 && leaf.x1 <= edit.x0 + 1.0,
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

#[test]
fn clicking_outside_address_bar_exits_edit_mode() {
    let supervisor = Supervisor::new(false).expect("test runtime");
    let bg_scope = supervisor.scope();
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().join("work");
    std::fs::create_dir_all(&dir).expect("create work directory");

    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);

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
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        let buttons = collect_live_buttons(&offscreen);
        let edit = find_button_by_label(&buttons, "Edit location")
            .expect("edit location button")
            .clone();
        let leaf = buttons
            .iter()
            .find(|btn| btn.label == "work" && btn.y0 < 60.0)
            .expect("current leaf breadcrumb")
            .clone();

        let assert_editing_and_focused = |browser: &Browser, context: &str| {
            assert!(
                matches!(
                    browser.0.address_presentation.get_untracked(),
                    crate::pages::workspace::model::AddressPresentation::Editing { .. }
                ),
                "{context}: expected address edit mode"
            );
            let input_id = browser
                .0
                .address_node
                .get_untracked()
                .expect("address input node bound in edit mode");
            let focused = browser
                .0
                .owner
                .with(|| zgui::view::current_host().and_then(|h| h.focused().get_untracked()));
            assert_eq!(
                focused,
                Some(input_id),
                "{context}: address input must hold focus"
            );
        };

        // 1. Click Edit location button (press & release in separate frames like a real user).
        click_at(
            &mut harness,
            edit.center_x() as f32,
            edit.center_y() as f32,
            100,
        );
        assert_editing_and_focused(&browser, "after clicking Edit location");

        // 2. Clicking inside the address input itself keeps edit mode open.
        click_at(
            &mut harness,
            (edit.x0 - 40.0) as f32,
            edit.center_y() as f32,
            250,
        );
        assert_editing_and_focused(&browser, "after clicking inside address input");

        // 3. Click outside onto the blank file-list / marquee area.
        click_at(&mut harness, 700.0, 500.0, 400);
        assert!(
            matches!(
                browser.0.address_presentation.get_untracked(),
                crate::pages::workspace::model::AddressPresentation::Breadcrumbs
            ),
            "clicking blank file-list area outside the address bar must exit edit mode"
        );

        // 4. Click directly on the address bar blank area to re-enter edit mode.
        click_at(
            &mut harness,
            (edit.x0 - 30.0) as f32,
            edit.center_y() as f32,
            600,
        );
        assert_editing_and_focused(&browser, "after clicking address bar blank area");

        // 5. Click outside onto the sidebar blank area.
        click_at(&mut harness, 80.0, 500.0, 800);
        assert!(
            matches!(
                browser.0.address_presentation.get_untracked(),
                crate::pages::workspace::model::AddressPresentation::Breadcrumbs
            ),
            "clicking sidebar area outside the address bar must exit edit mode"
        );

        // 6. Click current leaf breadcrumb to enter edit mode, then click toolbar drag strip.
        click_at(
            &mut harness,
            leaf.center_x() as f32,
            leaf.center_y() as f32,
            1000,
        );
        assert_editing_and_focused(&browser, "after clicking current leaf breadcrumb");

        click_at(&mut harness, 100.0, edit.center_y() as f32, 1200);
        assert!(
            matches!(
                browser.0.address_presentation.get_untracked(),
                crate::pages::workspace::model::AddressPresentation::Breadcrumbs
            ),
            "clicking toolbar drag area outside the address bar must exit edit mode"
        );

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::system())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
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
                    start: dir.clone(),
                    home: dir.clone(),
                    picker: None,
                },
                volumes::Service::new(scope),
                Rc::new(|_| {}),
            );
            *slot_for_view.borrow_mut() = Some(browser.clone());
            browser.view()
        })
        .expect("headless address outside-click harness");
}
