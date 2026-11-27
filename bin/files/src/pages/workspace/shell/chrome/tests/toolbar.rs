use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};

use gds::{
    app::Gds,
    appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH},
    i18n::Language,
    service::task_manager::{Scope, Supervisor},
};
use zgui::{
    app::Fonts,
    geom::{DevicePx, Size},
    platform::{AppHandler, PlatformError, SurfaceEvent},
    prelude::*,
    reactive::{Mounted, flush, install},
    view,
    vocab::{KeyCode, KeyEvent, Modifiers, NamedKey, PhysicalKey},
};
use zgui_platform_headless::Harness;

use super::super::{Breadcrumb, Chrome};
use super::{
    clear_recorded_tab_nodes, click_at, collect_live_buttons, find_button_by_label, noop_renderer,
    run_browser_harness,
};
use crate::pages::workspace::shell::view::tests::send_key_chord;
use crate::{
    entities::{
        location::{Location, places::Places, scheme::TagAddress, volumes},
        preferences,
    },
    pages::workspace::{
        navigation::Navigator,
        panes::Inspection,
        projection::{Geometry, UiState},
        shell::Browser,
        state::LaunchOptions,
    },
    shared::i18n,
};

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
                    {Chrome(&browser).address_bar()}
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
                    crate::pages::workspace::state::model::AddressPresentation::Editing { .. }
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
                crate::pages::workspace::state::model::AddressPresentation::Breadcrumbs
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
                crate::pages::workspace::state::model::AddressPresentation::Breadcrumbs
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
                crate::pages::workspace::state::model::AddressPresentation::Breadcrumbs
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
                Geometry(browser).width_tier_untracked(),
                gds::ui::window::WidthTier::Wide
            );
            assert_eq!(
                Geometry(browser).height_tier_untracked(),
                gds::ui::window::HeightTier::Normal
            );
            Inspection(browser).toggle_details();
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
                Geometry(browser).width_tier_untracked(),
                gds::ui::window::WidthTier::Compact
            );
            assert!(!Geometry(browser).details_permitted_untracked());
            assert_eq!(browser.sidebar_width().get_untracked(), SIDEBAR_WIDTH);
            UiState(browser).toggle_sidebar();
            flush();
            harness.settle(8);
            assert_eq!(
                browser.sidebar_width().get_untracked(),
                COLLAPSED_SIDEBAR_WIDTH
            );
            UiState(browser).toggle_sidebar();
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
                Geometry(browser).height_tier_untracked(),
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
            Navigator(browser).filter("alpha".to_owned());
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
                (Geometry(browser).list_row_height_untracked() - 40.0 * expected_scale).abs()
                    < 0.05,
                "expected scaled 40px list row height, got {}",
                Geometry(browser).list_row_height_untracked()
            );
            assert!(
                (browser
                    .0
                    .owner
                    .with(|| Geometry(browser).list_header_height())
                    - 30.0 * expected_scale)
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

    use zgui::{
        render::{RenderTarget, Renderer},
        runtime::AppError,
    };
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
        Navigator(&browser).open(Location::Volumes);
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

#[cfg(feature = "files-e2e")]
#[test]
fn extensions_page_renders_tags_and_external_thumbnailers_cleanly() {
    use std::path::PathBuf;

    use zgui::{
        render::{RenderTarget, Renderer},
        runtime::AppError,
        vocab::Role,
    };
    use zgui_render_wgpu::{SharedGraphics, wgpu};

    use super::click_button;

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
            UiState(&browser).toggle_settings();
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

            let preview_size = (1100.0, 1280.0);
            if let Some(dir) = &preview_dir {
                let window_guard = handle_driver.borrow();
                if let Some(window) = window_guard.as_ref() {
                    let snap_browser = browser.clone();
                    if let Ok(rendered) = window.render_view(
                        zgui::prelude::ViewSnapshotOptions::new()
                            .with_max_size(preview_size.0, preview_size.1),
                        move || {
                            view! {
                                box(
                                    style:width = format!("{}px", preview_size.0),
                                    style:height = format!("{}px", preview_size.1)
                                ) {
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

            if use_bwrap {
                harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                    DevicePx(1100.0),
                    DevicePx(1280.0),
                )));
                harness.settle(12);
                let live_buttons = collect_live_buttons(&surface);
                if let Some(disclosure_btn) = live_buttons
                    .iter()
                    .find(|btn| btn.y0 > thumb_btn.y1 + 20.0 && (btn.x1 - btn.x0) > 400.0)
                    .cloned()
                {
                    let collect_live_state = || {
                        let mut live_nodes = std::collections::HashMap::new();
                        for update in surface.a11y_log() {
                            for (id, node) in update.nodes {
                                live_nodes.insert(id, node);
                            }
                        }
                        let expanded_count = live_nodes
                            .values()
                            .filter(|n| n.role() == Role::Button && n.is_expanded() == Some(true))
                            .count();
                        let all_text = live_nodes
                            .values()
                            .filter_map(|n| n.value().or_else(|| n.label()))
                            .collect::<Vec<_>>()
                            .join("\n");
                        (expanded_count, all_text)
                    };
                    let (before_expanded, _) = collect_live_state();
                    click_at(
                        &mut harness,
                        disclosure_btn.center_x() as f32,
                        disclosure_btn.center_y() as f32,
                        100,
                    );
                    harness.advance(Duration::from_millis(200));
                    harness.settle(16);

                    let (after_expanded, all_text) = collect_live_state();
                    assert_eq!(
                        after_expanded,
                        before_expanded + 1,
                        "clicking the discovered thumbnailers disclosure must expand it"
                    );
                    assert!(
                        !all_text.contains("application/vnd.") && !all_text.contains("audio/x-pn-"),
                        "expanded discovered thumbnailers list must show human-readable format summaries, not raw MIME soup"
                    );

                    if let Some(dir) = &preview_dir {
                        let window_guard = handle_driver.borrow();
                        if let Some(window) = window_guard.as_ref() {
                            let snap_browser = browser.clone();
                            if let Ok(rendered) = window.render_view(
                                zgui::prelude::ViewSnapshotOptions::new()
                                    .with_max_size(1100.0, 1520.0),
                                move || {
                                    view! {
                                        box(style:width = "1100px", style:height = "1520px") {
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
                                img.save(dir.join("extensions-page-expanded.png"))
                                    .expect("save expanded extensions preview png");
                            }
                        }
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

#[cfg(all(feature = "files-e2e", target_os = "linux"))]
#[test]
fn enabling_external_thumbnailers_refreshes_video_thumbnails_in_grid_and_details() {
    use std::process::Command;

    use gds::ui::file_inspection::State as InspectionState;
    use zgui::{
        render::{RenderTarget, Renderer},
        runtime::AppError,
        vocab::Role,
    };
    use zgui_render_wgpu::{SharedGraphics, wgpu};

    use super::{click_button, collect_live_role};

    let has_tool = |name: &str| {
        Command::new(name)
            .env_remove("LD_LIBRARY_PATH")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
            || Command::new(name)
                .env_remove("LD_LIBRARY_PATH")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
    };
    if !has_tool("ffmpeg") || !has_tool("bwrap") || !has_tool("ffmpegthumbnailer") {
        return;
    }

    let graphics = SharedGraphics::new();
    if graphics.open_gpu().is_err() {
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let start = temp.path().to_path_buf();
    let video_path = start.join("clip.mp4");
    let generated = Command::new("ffmpeg")
        .env_remove("LD_LIBRARY_PATH")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=0.5:size=320x240:rate=10",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&video_path)
        .status()
        .expect("spawn ffmpeg");
    assert!(
        generated.success() && video_path.is_file(),
        "ffmpeg fixture generation must succeed"
    );

    let window_graphics = graphics.clone();
    let offscreen_graphics = graphics.clone();
    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let start_driver = start.clone();

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        if let Some(offscreen) = harness.platform().offscreens().first() {
            offscreen.set_a11y_listener(true);
        }
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(920.0),
        )));
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        // Wait for directory load so clip.mp4 is listed.
        let load_deadline = std::time::Instant::now() + Duration::from_secs(5);
        while browser.read_untracked(|state| state.tab().rows.is_empty()) {
            assert!(
                std::time::Instant::now() < load_deadline,
                "timed out waiting for temp dir listing"
            );
            harness.advance(Duration::from_millis(20));
            harness.pump();
            flush();
            harness.settle(4);
            std::thread::sleep(Duration::from_millis(10));
        }

        crate::pages::workspace::panes::inspection::tests::clear_recorded_artwork_nodes();
        Navigator(&browser).toggle_view();
        Inspection(&browser).toggle_details();
        crate::pages::workspace::shell::keyboard::Keyboard(&browser).select_row(0, Modifiers::NONE);
        flush();
        harness.settle(16);

        assert!(
            crate::pages::workspace::panes::inspection::tests::tile_thumb_node("clip.mp4")
                .is_none(),
            "video thumbnail must not exist before external thumbnailers are enabled"
        );
        assert!(
            crate::pages::workspace::panes::inspection::tests::details_fallback_node().is_some(),
            "details panel must start on fallback icon before external thumbnailers are enabled"
        );

        // Open Settings -> Extensions, wait for bwrap probe, and click the Enable switch.
        UiState(&browser).toggle_settings();
        flush();
        harness.settle(16);

        let surface = harness.platform().offscreens()[0].clone();
        let ext_label = gds::i18n::text_untracked("settings-extensions");
        click_button(&mut harness, &ext_label);
        let probe_deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut clicked_switch = false;
        while std::time::Instant::now() < probe_deadline {
            harness.advance(Duration::from_millis(30));
            harness.pump();
            flush();
            harness.settle(8);
            let buttons = collect_live_buttons(&surface);
            let tags_btn =
                find_button_by_label(&buttons, &gds::i18n::text_untracked("tags-detect"));
            let thumb_btn =
                find_button_by_label(&buttons, &gds::i18n::text_untracked("thumbnailers-detect"));
            if let (Some(tags_btn), Some(thumb_btn)) = (tags_btn, thumb_btn) {
                let switches = collect_live_role(&surface, Role::Switch);
                let mut candidates: Vec<_> = switches
                    .into_iter()
                    .filter(|(sw, disabled)| {
                        !disabled && sw.y0 > tags_btn.y1 && sw.y1 < thumb_btn.y0
                    })
                    .collect();
                candidates.sort_by(|a, b| a.0.y0.total_cmp(&b.0.y0));
                if let Some((enable_sw, _)) = candidates.first() {
                    click_at(
                        &mut harness,
                        enable_sw.center_x() as f32,
                        enable_sw.center_y() as f32,
                        100,
                    );
                    harness.settle(12);
                    clicked_switch = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        assert!(
            clicked_switch,
            "enable external thumbnailers switch must become enabled after bwrap probe"
        );
        assert!(
            browser.0.preferences.snapshot().thumbnailers.enabled,
            "clicking the Enable external thumbnailers switch must enable thumbnailers in preferences"
        );

        // Return to directory view and wait for sandboxed ffmpegthumbnailer to populate both Grid & Details.
        crate::pages::workspace::panes::inspection::tests::clear_recorded_artwork_nodes();
        UiState(&browser).toggle_settings();
        flush();
        harness.settle(12);

        let clip_source = browser.read_untracked(|state| {
            state
                .tab()
                .rows
                .iter()
                .find_map(|row| match row {
                    crate::pages::workspace::state::model::Row::File(entry)
                        if entry.path == start_driver.join("clip.mp4") =>
                    {
                        Some(entry.inspection_source())
                    }
                    _ => None,
                })
                .expect("clip.mp4 row present")
        });

        let thumb_deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut ready = false;
        while std::time::Instant::now() < thumb_deadline {
            harness.advance(Duration::from_millis(30));
            harness.pump();
            flush();
            harness.settle(6);
            let cache = browser.0.inspections.get_untracked();
            let peek_ready = Signal::derive_local({
                let cache = cache.clone();
                let clip_source = clip_source.clone();
                move || {
                    cache
                        .peek(&clip_source)
                        .is_some_and(|snap| matches!(snap.thumbnail, InspectionState::Ready(_)))
                }
            })
            .get_untracked();
            if peek_ready
                && crate::pages::workspace::panes::inspection::tests::tile_thumb_node("clip.mp4")
                    .is_some()
                && crate::pages::workspace::panes::inspection::tests::details_thumb_node().is_some()
                && crate::pages::workspace::panes::inspection::tests::details_fallback_node()
                    .is_none()
            {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            ready,
            "enabling external thumbnailers must generate and display clip.mp4 thumbnail in both Grid view and Details panel (external={:?}, tile={:?}, details={:?}, details_fallback={:?}, thumb_state={:?})",
            browser
                .0
                .external_thumbnailers
                .borrow()
                .as_ref()
                .map(|e| e.entry_count()),
            crate::pages::workspace::panes::inspection::tests::tile_thumb_node("clip.mp4")
                .is_some(),
            crate::pages::workspace::panes::inspection::tests::details_thumb_node().is_some(),
            crate::pages::workspace::panes::inspection::tests::details_fallback_node().is_some(),
            Signal::derive_local({
                let cache = browser.0.inspections.get_untracked();
                let clip_source = clip_source.clone();
                move || {
                    cache.peek(&clip_source).map(|snap| match snap.thumbnail {
                        InspectionState::NotRequested => "NotRequested".to_owned(),
                        InspectionState::Loading => "Loading".to_owned(),
                        InspectionState::Ready(_) => "Ready".to_owned(),
                        InspectionState::Unavailable => "Unavailable".to_owned(),
                        InspectionState::TooLarge => "TooLarge".to_owned(),
                        InspectionState::Failed(err) => format!("Failed({err})"),
                    })
                }
            })
            .get_untracked(),
        );

        gds::service::file_inspection::invalidate_thumbnail_cache(&start_driver.join("clip.mp4"));
        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::system())
        .with_size(1280.0, 920.0)
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
            let initial = preferences::Preferences::default();
            let _prefs =
                preferences::Service::install(initial, &scope).expect("preferences service");
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
        .expect("video thumbnail e2e test ran");
}
