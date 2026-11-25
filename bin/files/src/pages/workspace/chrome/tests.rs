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

use super::{
    Breadcrumb, CollapsibleMeta, CollapsibleSection, TabCycleTargets, allocate_collapsible_heights,
    compute_tab_cycle_targets, default_accordion_section, sidebar_accordion_threshold,
};
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
    assert!(!crumbs[0].scheme);
    assert!(!crumbs[0].current);
    assert!(!crumbs[0].separated);
    assert_eq!(crumbs[1].label, "mnt/");
    assert_eq!(crumbs[1].location, Some(Location::from(Path::new("/mnt"))));
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
        assert_eq!(crumbs[0].label, "Tags");
        assert!(crumbs[0].scheme);
        assert_eq!(crumbs[0].icon, Some(gds::ui::IconName::Tag));
        assert_eq!(crumbs[0].location, Some(Location::Tags(root_tag)));
        assert!(!crumbs[0].current);
        assert!(!crumbs[0].separated);

        assert_eq!(crumbs[1].label, "work");
        assert!(!crumbs[1].scheme);
        assert_eq!(crumbs[1].location, Some(Location::Tags(work_tag)));
        assert!(!crumbs[1].current);
        assert!(crumbs[1].separated);

        assert_eq!(crumbs[2].label, "urgent");
        assert_eq!(crumbs[2].location, Some(Location::Tags(leaf)));
        assert!(crumbs[2].current);
        assert!(crumbs[2].separated);
    });
    root.unmount();
}

#[test]
fn virtual_location_and_remote_breadcrumbs_render_localized_scheme_chips() {
    let _ = install();
    let root = Mounted::new();
    root.with(|| {
        let app = Gds::install(Scope::default());
        app.i18n()
            .install_catalog(i18n::prepare(Language::Chinese).expect("Chinese catalog"));

        let vol_crumbs = Breadcrumb::for_location(&Location::Volumes, None);
        assert_eq!(vol_crumbs.len(), 2);
        assert_eq!(vol_crumbs[0].label, "位置");
        assert!(vol_crumbs[0].scheme);
        assert_eq!(vol_crumbs[0].location, None);
        assert_eq!(vol_crumbs[0].icon, Some(gds::ui::IconName::Bookmark));
        assert_eq!(vol_crumbs[1].label, "卷");
        assert!(!vol_crumbs[1].scheme);
        assert_eq!(vol_crumbs[1].location, Some(Location::Volumes));
        assert!(vol_crumbs[1].current);
        assert!(vol_crumbs[1].separated);

        let zh_net = Breadcrumb::for_location(&Location::Network, None);
        assert_eq!(zh_net.len(), 1);
        assert_eq!(zh_net[0].label, "网络");
        assert!(zh_net[0].scheme);
        assert_eq!(zh_net[0].icon, Some(gds::ui::IconName::Network));
        assert_eq!(zh_net[0].location, Some(Location::Network));
        assert!(zh_net[0].current);
        assert!(!zh_net[0].separated);

        let zh_trash = Breadcrumb::for_location(&Location::Trash, None);
        assert_eq!(zh_trash.len(), 1);
        assert_eq!(zh_trash[0].label, "回收站");
        assert!(zh_trash[0].scheme);
        assert_eq!(zh_trash[0].icon, Some(gds::ui::IconName::Trash));
        assert_eq!(zh_trash[0].location, Some(Location::Trash));
        assert!(zh_trash[0].current);
        assert!(!zh_trash[0].separated);

        app.i18n()
            .install_catalog(i18n::prepare(Language::English).expect("English catalog"));
        let fav_crumbs = Breadcrumb::for_location(&Location::Favorites, None);
        assert_eq!(fav_crumbs.len(), 2);
        assert_eq!(fav_crumbs[0].label, "Location");
        assert!(fav_crumbs[0].scheme);
        assert_eq!(fav_crumbs[1].label, "Favorites");
        assert!(fav_crumbs[1].current);

        let recent_crumbs = Breadcrumb::for_location(&Location::Recent, None);
        assert_eq!(recent_crumbs.len(), 1);
        assert_eq!(recent_crumbs[0].label, "Recent");
        assert!(recent_crumbs[0].scheme);
        assert_eq!(recent_crumbs[0].icon, Some(gds::ui::IconName::Clock));
        assert_eq!(recent_crumbs[0].location, Some(Location::Recent));
        assert!(recent_crumbs[0].current);
        assert!(!recent_crumbs[0].separated);

        let net_crumbs = Breadcrumb::for_location(&Location::Network, None);
        assert_eq!(net_crumbs.len(), 1);
        assert_eq!(net_crumbs[0].label, "Network");
        assert!(net_crumbs[0].scheme);
        assert_eq!(net_crumbs[0].icon, Some(gds::ui::IconName::Network));
        assert!(net_crumbs[0].current);

        let trash_crumbs = Breadcrumb::for_location(&Location::Trash, None);
        assert_eq!(trash_crumbs.len(), 1);
        assert_eq!(trash_crumbs[0].label, "Trash");
        assert!(trash_crumbs[0].scheme);
        assert_eq!(trash_crumbs[0].icon, Some(gds::ui::IconName::Trash));
        assert!(trash_crumbs[0].current);

        let remote = gds::service::remote::Address::parse("sftp://nas.local/home/atlas/docs")
            .expect("valid sftp address");
        let remote_crumbs = Breadcrumb::for_location(&Location::Remote(remote), None);
        assert_eq!(remote_crumbs.len(), 5);
        assert_eq!(remote_crumbs[0].label, "SFTP");
        assert!(remote_crumbs[0].scheme);
        assert_eq!(remote_crumbs[0].location, None);
        assert_eq!(remote_crumbs[1].label, "nas.local");
        assert_eq!(remote_crumbs[2].label, "home");
        assert_eq!(remote_crumbs[3].label, "atlas");
        assert_eq!(remote_crumbs[4].label, "docs");
        assert!(remote_crumbs[4].current);
        assert!(remote_crumbs[1..].iter().all(|c| c.separated && !c.scheme));
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
        browser.0.window_width.set(540.0);
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
        browser.0.window_width.set(540.0);
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

#[test]
fn orthogonal_responsive_tiers_and_proportional_list_scaling_contracts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let start = temp.path().join("workspace");
    std::fs::create_dir_all(&start).expect("create workspace");
    std::fs::write(start.join("alpha.txt"), b"a\n").expect("write alpha");
    std::fs::write(start.join("bravo.txt"), b"b\n").expect("write bravo");
    clear_recorded_tab_nodes();

    run_browser_harness(
        &start,
        (1240.0, 760.0),
        volumes::Snapshot::default(),
        |browser| AnyView::new(browser.view()),
        |harness, browser| {
            let surface = harness.platform().offscreens()[0].clone();
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while browser.read_untracked(|ws| ws.tab().rows.len() < 2) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out loading fixture rows"
                );
                harness.advance(Duration::from_millis(25));
                harness.pump();
            }
            harness.settle(8);

            // 1. Wide x Normal (1240 x 760): Details toggles cleanly; Locations starts expanded.
            assert_eq!(
                browser.width_tier_untracked(),
                gds::ui::window::WidthTier::Wide
            );
            assert_eq!(
                browser.height_tier_untracked(),
                gds::ui::window::HeightTier::Normal
            );
            browser.toggle_details();
            flush();
            harness.settle(8);
            assert!(browser.0.details.get_untracked());
            let loc_bounds_normal = browser
                .0
                .locations_node
                .window_bounds()
                .expect("expanded locations bounds in Normal height");
            assert!(loc_bounds_normal.size.height.0 > 60.0);

            // 2. Compact x Normal (960 x 760): Details is suppressed while Sidebar stays toggleable.
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(960.0),
                DevicePx(760.0),
            )));
            harness.advance(Duration::from_millis(25));
            harness.settle(12);
            assert_eq!(
                browser.width_tier_untracked(),
                gds::ui::window::WidthTier::Compact
            );
            assert!(!browser.details_permitted_untracked());
            assert_eq!(browser.sidebar_width().get_untracked(), SIDEBAR_WIDTH);
            browser.toggle_sidebar();
            flush();
            harness.settle(8);
            assert_eq!(
                browser.sidebar_width().get_untracked(),
                COLLAPSED_SIDEBAR_WIDTH
            );
            browser.toggle_sidebar();
            flush();
            harness.settle(8);
            assert_eq!(browser.sidebar_width().get_untracked(), SIDEBAR_WIDTH);

            // 3. Compact x Micro (960 x 440): Locations auto-collapses without mutating preferences,
            //    toolbar exposes `page-actions` (`...`), and Ctrl+F opens + Esc collapses the 24px
            //    footer filter.
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(960.0),
                DevicePx(440.0),
            )));
            harness.advance(Duration::from_millis(25));
            harness.settle(12);
            assert_eq!(
                browser.height_tier_untracked(),
                gds::ui::window::HeightTier::Micro
            );
            assert!(
                !browser
                    .0
                    .preferences
                    .snapshot()
                    .files
                    .sidebar
                    .locations_collapsed,
                "Micro height auto-collapse must not mutate persisted locations_collapsed preference"
            );
            let buttons_micro = collect_live_buttons(&surface);
            let page_menu_btn =
                find_button_by_label(&buttons_micro, &gds::i18n::text_untracked("page-tools"))
                    .expect("toolbar trailing ... page-tools button in Micro height");
            assert!(
                page_menu_btn.y1 <= 48.0,
                "expected ... page-actions button inside 48px toolbar when H < 520, got {page_menu_btn:?}"
            );
            let filter_btn =
                find_button_by_label(&buttons_micro, &gds::i18n::text_untracked("filter"))
                    .expect("24px footer Search trigger button in Micro height");
            assert!(
                (filter_btn.y1 - filter_btn.y0 - 24.0).abs() <= 1.0 && filter_btn.y0 >= 408.0,
                "expected 24px subordinate filter button inside 32px footer, got {filter_btn:?}"
            );

            browser.focus_content();
            harness.settle(4);
            send_key_chord(harness, KeyEvent::character("f"), Modifiers::CONTROL);
            assert!(
                browser.0.footer_filter_open.get_untracked() && browser.0.filter_node.is_bound(),
                "Ctrl+F in Micro height must expand and bind the inline footer filter"
            );
            browser.filter("alpha".to_owned());
            flush();
            harness.settle(6);
            assert_eq!(browser.read_untracked(|ws| ws.tab().rows.len()), 1);

            send_key_chord(
                harness,
                KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                Modifiers::NONE,
            );
            assert!(
                browser.0.filter.get_untracked().is_empty()
                    && !browser.0.footer_filter_open.get_untracked(),
                "Escape in Micro height must clear filter text and collapse the footer filter"
            );

            // 4. Proportional list scaling at 20px font size (`scale = 20 / DEFAULT_SIZE`).
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1240.0),
                DevicePx(760.0),
            )));
            harness.advance(Duration::from_millis(25));
            browser.0.owner.with(|| {
                Gds::current()
                    .appearance()
                    .typography()
                    .apply(&gds::appearance::typography::Preferences {
                        family: None,
                        size: 20.0,
                    })
                    .expect("valid 20px font size");
            });
            flush();
            harness.settle(12);
            let expected_scale = 20.0_f32 / gds::appearance::typography::DEFAULT_SIZE;
            assert!(
                (browser.list_row_height_untracked() - 40.0 * expected_scale).abs() < 0.05,
                "expected scaled 40px list row height, got {}",
                browser.list_row_height_untracked()
            );
            assert!(
                (browser.0.owner.with(|| browser.list_header_height()) - 30.0 * expected_scale)
                    .abs()
                    < 0.05,
                "expected scaled 30px list header height"
            );
        },
    );
}

#[cfg(feature = "files-e2e")]
#[test]
fn volumes_page_removes_inline_unmount_buttons_aligns_mount_column_and_renders_schema_chip() {
    use std::path::PathBuf;

    use zgui_render_wgpu::{SharedGraphics, wgpu};

    let graphics = SharedGraphics::new();
    if graphics.open_gpu().is_err() {
        return;
    }
    let window_graphics = graphics;

    let temp = tempfile::tempdir().expect("tempdir");
    let start = temp.path().to_path_buf();
    let preview_dir = std::env::var_os("GDS_VOLUMES_PREVIEW_DIR").map(PathBuf::from);
    let snapshot = volumes::Snapshot {
        drives: vec![
            volumes::Drive {
                id: "drive-nvme".into(),
                name: "NVMe".into(),
                kind: volumes::DeviceKind::Disk,
                can_power_off: false,
                can_eject: false,
                sibling_id: None,
            },
            volumes::Drive {
                id: "drive-usb".into(),
                name: "USB".into(),
                kind: volumes::DeviceKind::Usb,
                can_power_off: true,
                can_eject: false,
                sibling_id: None,
            },
        ],
        volumes: vec![
            volumes::Volume {
                id: volumes::VolumeId {
                    stable: Some("uuid-root".into()),
                    attachment: "nvme0n1p2".into(),
                },
                object: "/org/freedesktop/UDisks2/block_devices/nvme0n1p2".into(),
                drive: Some("drive-nvme".into()),
                name: "root".into(),
                size: 847_800_000_000,
                label: "root".into(),
                filesystem: "ext4".into(),
                read_only: false,
                locked: false,
                mountable: true,
                system_mount: true,
                mount_points: vec![PathBuf::from("/"), PathBuf::from("/var")],
                visible: true,
            },
            volumes::Volume {
                id: volumes::VolumeId {
                    stable: Some("uuid-kioxia".into()),
                    attachment: "nvme1n1p1".into(),
                },
                object: "/org/freedesktop/UDisks2/block_devices/nvme1n1p1".into(),
                drive: Some("drive-nvme".into()),
                name: "KIOXIA-EXCERIA G2 SSD".into(),
                size: 1_863_000_000_000,
                label: "KIOXIA".into(),
                filesystem: "ntfs".into(),
                read_only: false,
                locked: false,
                mountable: true,
                system_mount: false,
                mount_points: vec![
                    PathBuf::from("/mnt/nvme1t8"),
                    PathBuf::from("/mnt/nvme-alt"),
                ],
                visible: true,
            },
            volumes::Volume {
                id: volumes::VolumeId {
                    stable: Some("uuid-n300".into()),
                    attachment: "sda1".into(),
                },
                object: "/org/freedesktop/UDisks2/block_devices/sda1".into(),
                drive: Some("drive-usb".into()),
                name: "N300-001".into(),
                size: 7_452_000_000_000,
                label: "N300-001".into(),
                filesystem: "ntfs".into(),
                read_only: false,
                locked: false,
                mountable: true,
                system_mount: false,
                mount_points: vec![PathBuf::from("/mnt/n300")],
                visible: true,
            },
            volumes::Volume {
                id: volumes::VolumeId {
                    stable: Some("uuid-unmounted".into()),
                    attachment: "sdb1".into(),
                },
                object: "/org/freedesktop/UDisks2/block_devices/sdb1".into(),
                drive: Some("drive-usb".into()),
                name: "BACKUP-USB".into(),
                size: 500_000_000_000,
                label: "BACKUP-USB".into(),
                filesystem: "ext4".into(),
                read_only: false,
                locked: false,
                mountable: true,
                system_mount: false,
                mount_points: Vec::new(),
                visible: true,
            },
        ],
    };

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let handle_slot: Rc<RefCell<Option<WindowHandle>>> = Rc::default();
    let handle_view = Rc::clone(&handle_slot);
    let handle_driver = Rc::clone(&handle_slot);
    let snapshot_for_view = snapshot;

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        if let Some(offscreen) = harness.platform().offscreens().first() {
            offscreen.set_a11y_listener(true);
        }
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1100.0),
            DevicePx(560.0),
        )));
        harness.settle(16);
        let browser = browser_slot.borrow().clone().expect("browser constructed");
        browser.open(Location::Volumes);
        flush();
        harness.settle(20);

        let surface = harness.platform().offscreens()[0].clone();
        let buttons = collect_live_buttons(&surface);

        if let Some(dir) = &preview_dir {
            let window_guard = handle_driver.borrow();
            if let Some(window) = window_guard.as_ref() {
                let snap_browser = browser.clone();
                if let Ok(rendered) = window.render_view(
                    zgui::prelude::ViewSnapshotOptions::new().with_max_size(1100.0, 560.0),
                    move || {
                        view! {
                            box(style:width = "1100px", style:height = "560px") {
                                {snap_browser.view()}
                            }
                        }
                    },
                ) {
                    let _ = std::fs::create_dir_all(dir);
                    let img = image::RgbaImage::from_raw(
                        rendered.image.width(),
                        rendered.image.height(),
                        rendered.image.rgba().to_vec(),
                    )
                    .expect("valid rgba");
                    img.save(dir.join("volumes-and-schema.png"))
                        .expect("save preview png");
                }
            }
        }

        // No inline mount or unmount buttons inside table rows (below the page header bar).
        let unmount_label = gds::i18n::text_untracked("device-unmount");
        let mount_label = gds::i18n::text_untracked("device-mount");
        let row_action_buttons: Vec<_> = buttons
            .iter()
            .filter(|b| b.y0 >= 120.0 && (b.label == unmount_label || b.label == mount_label))
            .collect();
        assert!(
            row_action_buttons.is_empty(),
            "Volumes table rows must not render inline mount/unmount buttons, found: {row_action_buttons:?}"
        );

        // Both multi-mount dropdown pickers must share the exact same left x coordinate (no
        // right-shift from space-between + hidden anchors).
        let row_pickers: Vec<_> = buttons
            .iter()
            .filter(|b| b.y0 >= 120.0 && b.y1 <= 500.0 && b.x0 >= 600.0)
            .collect();
        assert_eq!(
            row_pickers.len(),
            2,
            "expected exactly 2 multi-mount dropdown picker buttons in column 3, got: {row_pickers:?} (all buttons: {buttons:?})"
        );
        assert!(
            (row_pickers[0].x0 - row_pickers[1].x0).abs() <= 1.5,
            "multi-mount pickers must align along the same left edge: {row_pickers:?}"
        );
        assert!(
            row_pickers[0].x0 < 980.0,
            "root mount picker must be left-aligned inside column 3 rather than flushed to the right edge: {:?}",
            row_pickers[0]
        );

        // Address bar must display the localized "卷" crumb button (not raw `location:/volumes`).
        assert!(
            buttons.iter().any(|b| b.y1 <= 56.0 && b.label == "卷"),
            "expected localized '卷' breadcrumb in address bar, buttons: {buttons:?}"
        );
        assert!(
            !buttons
                .iter()
                .any(|b| b.y1 <= 56.0 && b.label.contains("location:/")),
            "address bar breadcrumb must not expose raw `location:/` URI"
        );

        harness.shut_down();
        Ok(())
    };

    let offscreen_graphics = window_graphics.clone();
    zgui::app()
        .with_fonts(Fonts::system())
        .with_size(1100.0, 560.0)
        .with_renderer(Box::new(move |_, target: RenderTarget| {
            let mut r = window_graphics.renderer_offscreen(
                target,
                wgpu::TextureFormat::Rgba8Unorm,
                false,
            )?;
            zgui_render_vector_vello::attach(&mut r, target.size);
            Ok::<_, AppError>(Box::new(r) as Box<dyn Renderer>)
        }))
        .with_offscreen_renderer(Box::new(move |target: RenderTarget| {
            let mut r = offscreen_graphics.renderer_offscreen(
                target,
                wgpu::TextureFormat::Rgba8Unorm,
                false,
            )?;
            zgui_render_vector_vello::attach(&mut r, target.size);
            Ok::<_, AppError>(Box::new(r) as Box<dyn Renderer>)
        }))
        .run_on(driver, move || {
            let scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(i18n::prepare(Language::Chinese).expect("Chinese catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
                .expect("preferences service");
            *handle_view.borrow_mut() = Some(use_window());
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(Places::default()),
                    start: start.clone(),
                    home: start.clone(),
                    picker: None,
                },
                volumes::Service::with_snapshot(scope, snapshot_for_view.clone()),
                Rc::new(|_| {}),
            );
            *slot_for_view.borrow_mut() = Some(browser.clone());
            browser.view()
        })
        .expect("volumes page GPU test ran");
}

#[test]
fn accordion_threshold_default_section_and_water_filling_allocator() {
    assert_eq!(sidebar_accordion_threshold(1), 0.0);
    assert!(sidebar_accordion_threshold(2) > 400.0);
    assert!(sidebar_accordion_threshold(3) > sidebar_accordion_threshold(2));

    // Prefer first visible user-expanded section with items.
    assert_eq!(
        default_accordion_section(
            None,
            CollapsibleMeta {
                visible: true,
                pref_expanded: true,
                has_items: true,
            },
            CollapsibleMeta {
                visible: false,
                pref_expanded: true,
                has_items: false,
            },
            CollapsibleMeta {
                visible: true,
                pref_expanded: true,
                has_items: true,
            },
        ),
        Some(CollapsibleSection::QuickAccess)
    );
    // When QuickAccess is empty and Volumes has drives, pick Volumes.
    assert_eq!(
        default_accordion_section(
            None,
            CollapsibleMeta {
                visible: true,
                pref_expanded: true,
                has_items: false,
            },
            CollapsibleMeta {
                visible: true,
                pref_expanded: true,
                has_items: true,
            },
            CollapsibleMeta {
                visible: true,
                pref_expanded: true,
                has_items: true,
            },
        ),
        Some(CollapsibleSection::Volumes)
    );
    // Focused section always wins when visible.
    assert_eq!(
        default_accordion_section(
            Some(CollapsibleSection::Locations),
            CollapsibleMeta {
                visible: true,
                pref_expanded: true,
                has_items: true,
            },
            CollapsibleMeta {
                visible: true,
                pref_expanded: true,
                has_items: true,
            },
            CollapsibleMeta {
                visible: true,
                pref_expanded: false,
                has_items: true,
            },
        ),
        Some(CollapsibleSection::Locations)
    );

    // Water-filling gives exact heights when total fits, and caps only oversized sections.
    assert_eq!(
        allocate_collapsible_heights(500.0, [156.0, 0.0, 276.0]),
        [156.0, 0.0, 276.0]
    );
    let capped = allocate_collapsible_heights(400.0, [150.0, 0.0, 400.0]);
    assert!((capped[0] - 150.0).abs() < 0.01);
    assert!((capped[2] - 250.0).abs() < 0.01);
    let equal = allocate_collapsible_heights(300.0, [250.0, 0.0, 250.0]);
    assert!((equal[0] - 150.0).abs() < 0.01);
    assert!((equal[2] - 150.0).abs() < 0.01);
}

#[test]
fn quick_access_current_tab_click_context_menu_tab_drag_reorder_and_accordion_mode() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let desktop = home.join("Desktop");
    let downloads = home.join("Downloads");
    let pictures = home.join("Pictures");
    let videos = home.join("Videos");
    let projects = home.join("Projects");
    for dir in [&desktop, &downloads, &pictures, &videos, &projects] {
        std::fs::create_dir_all(dir).expect("create fixture dir");
    }
    clear_recorded_tab_nodes();

    run_browser_harness(
        &home,
        (1240.0, 760.0),
        volumes::Snapshot::default(),
        |browser| AnyView::new(browser.view()),
        |harness, browser| {
            let surface = harness.platform().offscreens()[0].clone();

            // Seed default 4 Quick Access folders directly into the browser for deterministic test.
            for dir in [&desktop, &downloads, &pictures, &videos] {
                assert!(browser.add_quick_access_path(dir.clone()));
            }
            flush();
            harness.settle(12);

            // 1. Tall window (1240x760): Quick Access & Locations are BOTH expanded simultaneously.
            let qa_bounds_tall = browser
                .0
                .quick_access_node
                .window_bounds()
                .expect("expanded quick access bounds in tall window");
            let loc_bounds_tall = browser
                .0
                .locations_node
                .window_bounds()
                .expect("expanded locations bounds in tall window");
            assert!(
                qa_bounds_tall.size.height.0 > 120.0 && loc_bounds_tall.size.height.0 > 150.0,
                "expected both Quick Access ({:?}) and Locations ({:?}) expanded in tall window",
                qa_bounds_tall,
                loc_bounds_tall
            );

            // 2. Clicking a Quick Access item navigates in the CURRENT tab (matching Locations),
            //    while Ctrl+click opens a new tab.
            assert_eq!(browser.read_untracked(|ws| ws.tabs.len()), 1);
            let buttons = collect_live_buttons(&surface);
            let dl_btn = find_button_by_label(&buttons, "Downloads")
                .expect("Downloads quick access button")
                .clone();
            click_at(
                harness,
                dl_btn.center_x() as f32,
                dl_btn.center_y() as f32,
                100,
            );
            assert_eq!(
                browser.read_untracked(|ws| ws.tabs.len()),
                1,
                "clicking a directory in Quick Access must navigate in the current tab"
            );
            assert_eq!(
                browser.read_untracked(|ws| ws.tab().navigation.path().map(Path::to_path_buf)),
                Some(downloads.clone())
            );

            let buttons = collect_live_buttons(&surface);
            let desktop_btn = buttons
                .iter()
                .find(|b| b.label == "Desktop" && b.y0 >= qa_bounds_tall.origin.y.0 as f64 - 4.0)
                .expect("Desktop quick access row")
                .clone();
            click_at_with_modifiers(
                harness,
                desktop_btn.center_x() as f32,
                desktop_btn.center_y() as f32,
                PointerButton::Primary,
                Modifiers::CONTROL,
                200,
            );
            assert_eq!(
                browser.read_untracked(|ws| ws.tabs.len()),
                2,
                "Ctrl+clicking a Quick Access directory must open a new tab"
            );
            assert_eq!(
                browser.read_untracked(|ws| ws.tab().navigation.path().map(Path::to_path_buf)),
                Some(desktop.clone())
            );

            // 3. Heading has NO `+` button; adding the current folder lives in the Quick Access
            //    context menu (disabled on AlreadyAdded / NotLocal, enabled on CanAdd).
            let already_lbl = gds::i18n::text_untracked("quick-access-already-added");
            let add_lbl = gds::i18n::text_untracked("quick-access-add-current");
            let local_only_lbl = gds::i18n::text_untracked("quick-access-local-only");
            let buttons = collect_live_buttons(&surface);
            assert!(
                find_button_by_label(&buttons, &add_lbl).is_none()
                    && find_button_by_label(&buttons, &already_lbl).is_none()
                    && find_button_by_label(&buttons, &local_only_lbl).is_none(),
                "Quick Access heading must not render an inline `+` button"
            );

            let qa_heading = heading_node("quick-access").expect("quick-access heading node");
            let scale = qa_heading.scale().max(1.0);
            let h_bounds = qa_heading.window_bounds().expect("heading bounds");
            let hx = (h_bounds.origin.x.0 + h_bounds.size.width.0 * 0.5) / scale;
            let hy = (h_bounds.origin.y.0 + h_bounds.size.height.0 * 0.5) / scale;

            let escape = KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape));

            // AlreadyAdded on Desktop -> menu shows disabled already-added item.
            click_at_with_modifiers(
                harness,
                hx,
                hy,
                PointerButton::Secondary,
                Modifiers::NONE,
                260,
            );
            let items = collect_live_role(&surface, Role::MenuItem);
            assert!(
                items
                    .iter()
                    .any(|(item, disabled)| item.label == already_lbl && *disabled),
                "expected Quick Access context menu to show disabled already-added item on Desktop, got {items:?}"
            );
            send_key_chord(harness, escape.clone(), Modifiers::NONE);
            harness.settle(8);

            // NotLocal on location:/volumes -> menu shows disabled local-only item.
            browser.open(Location::Volumes);
            flush();
            harness.settle(8);
            click_at_with_modifiers(
                harness,
                hx,
                hy,
                PointerButton::Secondary,
                Modifiers::NONE,
                290,
            );
            let items = collect_live_role(&surface, Role::MenuItem);
            assert!(
                items
                    .iter()
                    .any(|(item, disabled)| item.label == local_only_lbl && *disabled),
                "expected Quick Access context menu to show disabled local-only item on Volumes, got {items:?}"
            );
            send_key_chord(harness, escape.clone(), Modifiers::NONE);
            harness.settle(8);

            // CanAdd on Projects -> clicking the menu item adds Projects to Quick Access.
            browser.open(Location::from(projects.as_path()));
            flush();
            harness.settle(8);
            click_at_with_modifiers(
                harness,
                hx,
                hy,
                PointerButton::Secondary,
                Modifiers::NONE,
                320,
            );
            let items = collect_live_role(&surface, Role::MenuItem);
            let (add_item, disabled) = items
                .iter()
                .find(|(item, _)| item.label == add_lbl)
                .expect("add current folder menu item in Quick Access context menu")
                .clone();
            assert!(
                !disabled,
                "add current folder menu item must be enabled on Projects"
            );
            click_at(
                harness,
                add_item.center_x() as f32,
                add_item.center_y() as f32,
                340,
            );
            assert!(
                browser.quick_access_paths_untracked().contains(&projects),
                "selecting Quick Access context menu item must add current local directory"
            );

            // Right-clicking Desktop -> Downloads -> Pictures -> heading back-to-back without
            // dismissing must ALWAYS keep exactly one context menu open (never stacked menus).
            let qa_bounds_now = browser
                .0
                .quick_access_node
                .window_bounds()
                .expect("current quick access bounds");
            let buttons = collect_live_buttons(&surface);
            let desktop_row = buttons
                .iter()
                .find(|b| b.label == "Desktop" && b.y0 >= qa_bounds_now.origin.y.0 as f64 - 4.0)
                .expect("Desktop quick access row")
                .clone();
            let dl_btn = buttons
                .iter()
                .find(|b| b.label == "Downloads" && b.y0 >= qa_bounds_now.origin.y.0 as f64 - 4.0)
                .expect("Downloads quick access row")
                .clone();
            let pic_row = buttons
                .iter()
                .find(|b| b.label == "Pictures" && b.y0 >= qa_bounds_now.origin.y.0 as f64 - 4.0)
                .expect("Pictures quick access row")
                .clone();
            for (step, (row_name, x, y, ts)) in [
                (
                    "Desktop",
                    desktop_row.x0 + 24.0,
                    desktop_row.center_y(),
                    355,
                ),
                ("Downloads", dl_btn.x0 + 24.0, dl_btn.center_y(), 360),
                ("Pictures", pic_row.x0 + 24.0, pic_row.center_y(), 365),
            ]
            .into_iter()
            .enumerate()
            {
                click_at_with_modifiers(
                    harness,
                    x as f32,
                    y as f32,
                    PointerButton::Secondary,
                    Modifiers::NONE,
                    ts,
                );
                let open_menus = collect_live_role(&surface, Role::Menu);
                assert_eq!(
                    open_menus.len(),
                    1,
                    "step {step} right-clicking {row_name} must replace prior menu with exactly 1 open menu, got {open_menus:?}"
                );
            }
            let item_entries = collect_live_role(&surface, Role::MenuItem);
            let remove_lbl = gds::i18n::text_untracked("quick-access-remove");
            assert!(
                item_entries.iter().any(|(i, _)| i.label == already_lbl)
                    && item_entries.iter().any(|(i, _)| i.label == remove_lbl),
                "Quick Access row menu must include add-current and remove entries, got {item_entries:?}"
            );
            // Switching directly from an open item menu to the section heading menu also replaces cleanly.
            let h_now = qa_heading.window_bounds().expect("current heading bounds");
            click_at_with_modifiers(
                harness,
                (h_now.origin.x.0 + h_now.size.width.0 * 0.5) / scale,
                (h_now.origin.y.0 + h_now.size.height.0 * 0.5) / scale,
                PointerButton::Secondary,
                Modifiers::NONE,
                368,
            );
            let open_menus = collect_live_role(&surface, Role::Menu);
            assert_eq!(
                open_menus.len(),
                1,
                "right-clicking section heading while item menu is open must leave exactly 1 menu, got {open_menus:?}"
            );
            let section_entries = collect_live_role(&surface, Role::MenuItem);
            assert_eq!(
                section_entries.len(),
                1,
                "section menu must replace item menu completely, got {section_entries:?}"
            );
            send_key_chord(harness, escape, Modifiers::NONE);
            harness.settle(8);
            assert!(
                collect_live_role(&surface, Role::Menu).is_empty(),
                "Escape must close the active context menu"
            );

            // Remove Projects so we can also test dragging a tab onto collapsed Quick Access!
            browser.remove_quick_access(&projects);
            flush();
            harness.settle(8);
            assert!(!browser.quick_access_paths_untracked().contains(&projects));

            // 4. Dragging a tab onto collapsed Quick Access adds the directory AND expands Quick Access.
            click_at(harness, hx, hy, 400);

            let active_tab_id = browser.read_untracked(|ws| ws.tab().id);
            let tab_node = recorded_tab_node(active_tab_id).expect("active tab node");
            let tab_bounds = tab_node.window_bounds().expect("active tab bounds");
            let qa_bounds_collapsed = qa_heading
                .window_bounds()
                .expect("collapsed heading bounds");
            let start = Point::new(
                CssPx((tab_bounds.origin.x.0 + tab_bounds.size.width.0 * 0.5) / scale),
                CssPx((tab_bounds.origin.y.0 + tab_bounds.size.height.0 * 0.5) / scale),
            );
            let mid = Point::new(CssPx(start.x.0), CssPx(start.y.0 + 18.0));
            let drop_pt = Point::new(
                CssPx(
                    (qa_bounds_collapsed.origin.x.0 + qa_bounds_collapsed.size.width.0 * 0.5)
                        / scale,
                ),
                CssPx(
                    (qa_bounds_collapsed.origin.y.0 + qa_bounds_collapsed.size.height.0 * 0.5)
                        / scale,
                ),
            );
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(start),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::from_origin(Duration::from_millis(500)),
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(start).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::from_origin(Duration::from_millis(510)),
            });
            harness.settle(4);
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(mid),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::from_origin(Duration::from_millis(530)),
            });
            harness.settle(4);
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(drop_pt),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::from_origin(Duration::from_millis(550)),
            });
            harness.settle(4);
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(drop_pt).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::from_origin(Duration::from_millis(570)),
            });
            harness.settle(12);
            assert!(
                browser.quick_access_paths_untracked().contains(&projects),
                "dropping a local tab onto Quick Access heading must add its directory"
            );
            assert!(
                !browser
                    .0
                    .preferences
                    .snapshot()
                    .files
                    .sidebar
                    .quick_access_collapsed,
                "dropping a tab onto collapsed Quick Access must expand it"
            );

            // 5. Reordering Quick Access items persists the new order.
            browser.reorder_quick_access_item(&projects, &desktop);
            flush();
            harness.settle(8);
            assert_eq!(
                browser.quick_access_paths_untracked().first(),
                Some(&projects),
                "reordered Quick Access item must move to target position"
            );

            // 6. Responsive vertical accordion mode: shrink window to 1240x500 (sidebar < 540px).
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1240.0),
                DevicePx(500.0),
            )));
            harness.advance(Duration::from_millis(25));
            harness.settle(16);
            assert!(
                browser.0.sidebar_in_accordion.get_untracked(),
                "short window must activate sidebar accordion mode"
            );
            // Initially Quick Access stays expanded while Locations collapses without mutating prefs.
            let qa_short = browser
                .0
                .quick_access_node
                .window_bounds()
                .expect("Quick Access stays expanded initially in accordion mode");
            assert!(qa_short.size.height.0 > 60.0);
            let loc_short_h = browser
                .0
                .locations_node
                .window_bounds()
                .map_or(0.0, |r| r.size.height.0);
            assert!(
                loc_short_h < 1.0,
                "Locations must collapse automatically in accordion mode, got height {loc_short_h}"
            );
            assert!(
                !browser
                    .0
                    .preferences
                    .snapshot()
                    .files
                    .sidebar
                    .locations_collapsed,
                "accordion mode must NOT mutate persisted locations_collapsed preference"
            );

            // Clicking Locations heading expands Locations and collapses Quick Access!
            let loc_heading = heading_node("locations").expect("locations heading node");
            let loc_h_bounds = loc_heading
                .window_bounds()
                .expect("locations heading bounds");
            click_at(
                harness,
                (loc_h_bounds.origin.x.0 + loc_h_bounds.size.width.0 * 0.5) / scale,
                (loc_h_bounds.origin.y.0 + loc_h_bounds.size.height.0 * 0.5) / scale,
                700,
            );
            let loc_after = browser
                .0
                .locations_node
                .window_bounds()
                .expect("Locations expanded after click in accordion mode");
            assert!(loc_after.size.height.0 > 60.0);
            let qa_after_h = browser
                .0
                .quick_access_node
                .window_bounds()
                .map_or(0.0, |r| r.size.height.0);
            assert!(
                qa_after_h < 1.0,
                "expanding Locations in accordion mode must collapse Quick Access, got height {qa_after_h}"
            );
            assert!(
                !browser
                    .0
                    .preferences
                    .snapshot()
                    .files
                    .sidebar
                    .quick_access_collapsed,
                "accordion switching must NOT mutate persisted quick_access_collapsed preference"
            );

            // Resizing back to tall window (1240x760) exits accordion mode and restores BOTH expanded!
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1240.0),
                DevicePx(760.0),
            )));
            harness.advance(Duration::from_millis(25));
            harness.settle(16);
            assert!(!browser.0.sidebar_in_accordion.get_untracked());
            let qa_restored = browser
                .0
                .quick_access_node
                .window_bounds()
                .expect("Quick Access restored in tall window");
            let loc_restored = browser
                .0
                .locations_node
                .window_bounds()
                .expect("Locations restored in tall window");
            assert!(
                qa_restored.size.height.0 > 120.0 && loc_restored.size.height.0 > 150.0,
                "resizing back to tall must restore both Quick Access ({qa_restored:?}) and Locations ({loc_restored:?})"
            );
        },
    );
}

#[cfg(feature = "files-e2e")]
#[test]
fn quick_access_sidebar_tall_and_accordion_gpu_preview() {
    use std::path::PathBuf;

    use zgui_render_wgpu::{SharedGraphics, wgpu};

    let graphics = SharedGraphics::new();
    if graphics.open_gpu().is_err() {
        return;
    }
    let window_graphics = graphics;

    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let desktop = home.join("桌面");
    let downloads = home.join("下载");
    let pictures = home.join("图片");
    let videos = home.join("视频");
    let work = home.join("项目");
    for dir in [&desktop, &downloads, &pictures, &videos, &work] {
        std::fs::create_dir_all(dir).expect("create dir");
    }
    let preview_dir = std::env::var_os("GDS_SIDEBAR_PREVIEW_DIR").map(PathBuf::from);

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let handle_slot: Rc<RefCell<Option<WindowHandle>>> = Rc::default();
    let handle_view = Rc::clone(&handle_slot);
    let handle_driver = Rc::clone(&handle_slot);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        if let Some(offscreen) = harness.platform().offscreens().first() {
            offscreen.set_a11y_listener(true);
        }
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1100.0),
            DevicePx(760.0),
        )));
        harness.settle(16);
        let browser = browser_slot.borrow().clone().expect("browser constructed");
        for dir in [&desktop, &downloads, &pictures, &videos] {
            browser.add_quick_access_path(dir.clone());
        }
        browser.new_tab(Some(Location::from(work.as_path())));
        flush();
        harness.settle(16);

        let save_snap = |name: &str, w: f32, h: f32| {
            if let Some(dir) = &preview_dir {
                let window_guard = handle_driver.borrow();
                if let Some(window) = window_guard.as_ref() {
                    let snap_browser = browser.clone();
                    if let Ok(rendered) = window.render_view(
                        zgui::prelude::ViewSnapshotOptions::new().with_max_size(w, h),
                        move || {
                            view! {
                                box(style:width = format!("{w:.0}px"), style:height = format!("{h:.0}px")) {
                                    {snap_browser.view()}
                                }
                            }
                        },
                    ) {
                        let _ = std::fs::create_dir_all(dir);
                        let img = image::RgbaImage::from_raw(
                            rendered.image.width(),
                            rendered.image.height(),
                            rendered.image.rgba().to_vec(),
                        )
                        .expect("valid rgba");
                        img.save(dir.join(name)).expect("save sidebar preview png");
                    }
                }
            }
        };

        // 1. Tall window: Tabs + 常用 + 位置 all expanded cleanly.
        assert!(!browser.0.sidebar_in_accordion.get_untracked());
        save_snap("sidebar-tall.png", 1100.0, 760.0);

        // 2. Short window (500px): Accordion mode -> 常用 expanded, 位置 collapsed.
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1100.0),
            DevicePx(500.0),
        )));
        harness.advance(Duration::from_millis(25));
        harness.settle(16);
        assert!(browser.0.sidebar_in_accordion.get_untracked());
        save_snap("sidebar-accordion-qa.png", 1100.0, 500.0);

        harness.shut_down();
        Ok(())
    };

    let offscreen_graphics = window_graphics.clone();
    zgui::app()
        .with_fonts(Fonts::system())
        .with_size(1100.0, 760.0)
        .with_renderer(Box::new(move |_, target: RenderTarget| {
            let mut r = window_graphics.renderer_offscreen(
                target,
                wgpu::TextureFormat::Rgba8Unorm,
                false,
            )?;
            zgui_render_vector_vello::attach(&mut r, target.size);
            Ok::<_, AppError>(Box::new(r) as Box<dyn Renderer>)
        }))
        .with_offscreen_renderer(Box::new(move |target: RenderTarget| {
            let mut r = offscreen_graphics.renderer_offscreen(
                target,
                wgpu::TextureFormat::Rgba8Unorm,
                false,
            )?;
            zgui_render_vector_vello::attach(&mut r, target.size);
            Ok::<_, AppError>(Box::new(r) as Box<dyn Renderer>)
        }))
        .run_on(driver, move || {
            let scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(i18n::prepare(Language::Chinese).expect("Chinese catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
                .expect("preferences service");
            *handle_view.borrow_mut() = Some(use_window());
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(Places::default()),
                    start: home.clone(),
                    home: home.clone(),
                    picker: None,
                },
                volumes::Service::new(scope),
                Rc::new(|_| {}),
            );
            *slot_for_view.borrow_mut() = Some(browser.clone());
            browser.view()
        })
        .expect("quick access sidebar GPU test ran");
}

#[cfg(feature = "files-e2e")]
#[test]
fn extensions_page_renders_tags_and_external_thumbnailers_cleanly() {
    use std::path::PathBuf;

    use zgui_render_wgpu::{SharedGraphics, wgpu};

    let graphics = SharedGraphics::new();
    if graphics.open_gpu().is_err() {
        return;
    }

    let preview_dir = std::env::var_os("GDS_EXTENSIONS_PREVIEW_DIR").map(PathBuf::from);
    let run_case = |use_bwrap: bool, preview_name: &str| -> f64 {
        let window_graphics = graphics.clone();
        let offscreen_graphics = graphics.clone();
        let temp = tempfile::tempdir().expect("tempdir");
        let start = temp.path().to_path_buf();
        let preview_dir = preview_dir.clone();
        let preview_name = preview_name.to_owned();

        let supervisor = Supervisor::new(false).expect("tokio supervisor");
        let bg_scope = supervisor.scope();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let slot_for_view = Rc::clone(&browser_slot);
        let handle_slot: Rc<RefCell<Option<WindowHandle>>> = Rc::default();
        let handle_view = Rc::clone(&handle_slot);
        let handle_driver = Rc::clone(&handle_slot);
        let thumb_y0: Rc<std::cell::Cell<f64>> = Rc::new(std::cell::Cell::new(0.0));
        let thumb_y0_driver = Rc::clone(&thumb_y0);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            if let Some(offscreen) = harness.platform().offscreens().first() {
                offscreen.set_a11y_listener(true);
            }
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1100.0),
                DevicePx(920.0),
            )));
            harness.settle(16);

            let browser = browser_slot.borrow().clone().expect("browser constructed");
            browser.toggle_settings();
            flush();
            harness.settle(16);

            let surface = harness.platform().offscreens()[0].clone();
            let ext_label = gds::i18n::text_untracked("settings-extensions");
            click_button(&mut harness, &ext_label);
            harness.advance(Duration::from_millis(200));
            harness.settle(20);

            let buttons = collect_live_buttons(&surface);
            let tags_detect = gds::i18n::text_untracked("tags-detect");
            let thumb_detect = gds::i18n::text_untracked("thumbnailers-detect");
            let tags_btn = find_button_by_label(&buttons, &tags_detect)
                .expect("Baloo tags re-check button on Extensions page");
            let thumb_btn = find_button_by_label(&buttons, &thumb_detect)
                .expect("External thumbnailers re-check button on Extensions page");
            assert!(
                thumb_btn.y0 > tags_btn.y1 + 20.0,
                "external thumbnailers section must appear cleanly below tags section: tags={tags_btn:?}, thumb={thumb_btn:?}"
            );
            thumb_y0_driver.set(thumb_btn.y0);

            if let Some(dir) = &preview_dir {
                let window_guard = handle_driver.borrow();
                if let Some(window) = window_guard.as_ref() {
                    let snap_browser = browser.clone();
                    if let Ok(rendered) = window.render_view(
                        zgui::prelude::ViewSnapshotOptions::new().with_max_size(1100.0, 920.0),
                        move || {
                            view! {
                                box(style:width = "1100px", style:height = "920px") {
                                    {snap_browser.view()}
                                }
                            }
                        },
                    ) {
                        let _ = std::fs::create_dir_all(dir);
                        let img = image::RgbaImage::from_raw(
                            rendered.image.width(),
                            rendered.image.height(),
                            rendered.image.rgba().to_vec(),
                        )
                        .expect("valid rgba");
                        img.save(dir.join(&preview_name))
                            .expect("save extensions preview png");
                    }
                }
            }

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::system())
            .with_size(1100.0, 920.0)
            .with_renderer(Box::new(move |_, target: RenderTarget| {
                let mut r = window_graphics.renderer_offscreen(
                    target,
                    wgpu::TextureFormat::Rgba8Unorm,
                    false,
                )?;
                zgui_render_vector_vello::attach(&mut r, target.size);
                Ok::<_, AppError>(Box::new(r) as Box<dyn Renderer>)
            }))
            .with_offscreen_renderer(Box::new(move |target: RenderTarget| {
                let mut r = offscreen_graphics.renderer_offscreen(
                    target,
                    wgpu::TextureFormat::Rgba8Unorm,
                    false,
                )?;
                zgui_render_vector_vello::attach(&mut r, target.size);
                Ok::<_, AppError>(Box::new(r) as Box<dyn Renderer>)
            }))
            .run_on(driver, move || {
                let scope = bg_scope.clone();
                let app = Gds::install(scope.clone());
                app.appearance().motion().set_enabled(false);
                let _ = app.appearance().install_window_sheet();
                app.i18n()
                    .install_catalog(i18n::prepare(Language::Chinese).expect("Chinese catalog"));
                let mut initial = preferences::Preferences::default();
                initial.thumbnailers.use_bwrap = use_bwrap;
                let _prefs =
                    preferences::Service::install(initial, &scope).expect("preferences service");
                *handle_view.borrow_mut() = Some(use_window());
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
                browser.view()
            })
            .expect("extensions page GPU test ran");

        thumb_y0.get()
    };

    let y_sandboxed = run_case(true, "extensions-page.png");
    let y_unsandboxed = run_case(false, "extensions-page-unsandboxed.png");
    assert!(
        y_unsandboxed > y_sandboxed + 16.0,
        "unsandboxed mode must render the warning banner above the probe row: sandboxed={y_sandboxed}, unsandboxed={y_unsandboxed}"
    );
}
