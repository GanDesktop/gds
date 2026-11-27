use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

use gds::appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH};
use zgui::{
    geom::{CssPx, DevicePx, Point, Size},
    platform::SurfaceEvent,
    prelude::*,
    reactive::flush,
    vocab::{
        KeyCode, KeyEvent, Modifiers, NamedKey, PhysicalKey, PointerAction, PointerButton,
        PointerEvent, Role, Timestamp,
    },
};
use zgui_platform_headless::OffscreenSurface;

use super::super::{
    CollapsibleMeta, CollapsibleSection, TabCycleTargets, allocate_collapsible_heights,
    compute_tab_cycle_targets, default_accordion_section, sidebar_accordion_threshold,
};
use super::{
    clear_recorded_tab_nodes, click_at, click_at_with_modifiers, click_button,
    collect_live_buttons, collect_live_role, find_button_by_label, heading_node, last_vector_inks,
    recorded_tab_node, recorded_tab_nodes, run_browser_harness, run_chrome_harness,
};
use crate::entities::location::{Location, volumes};
use crate::pages::workspace::{
    navigation::{Navigator, QuickAccess},
    shell::view::tests::send_key_chord,
};

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
        Navigator(browser).toggle_pin(0);
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
                assert!(QuickAccess(browser).add_quick_access_path(dir.clone()));
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
            Navigator(browser).open(Location::Volumes);
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
            Navigator(browser).open(Location::from(projects.as_path()));
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
                QuickAccess(browser)
                    .quick_access_paths_untracked()
                    .contains(&projects),
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
            QuickAccess(browser).remove_quick_access(&projects);
            flush();
            harness.settle(8);
            assert!(
                !QuickAccess(browser)
                    .quick_access_paths_untracked()
                    .contains(&projects)
            );

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
                QuickAccess(browser)
                    .quick_access_paths_untracked()
                    .contains(&projects),
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
            QuickAccess(browser).reorder_quick_access_item(&projects, &desktop);
            flush();
            harness.settle(8);
            assert_eq!(
                QuickAccess(browser).quick_access_paths_untracked().first(),
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
    use std::{cell::RefCell, path::PathBuf, rc::Rc};

    use gds::{app::Gds, i18n::Language, service::task_manager::Supervisor};
    use zgui::{
        app::Fonts,
        platform::{AppHandler, PlatformError},
        render::{RenderTarget, Renderer},
        runtime::AppError,
        view,
    };
    use zgui_platform_headless::Harness;
    use zgui_render_wgpu::{SharedGraphics, wgpu};

    use crate::{
        entities::{location::places::Places, preferences},
        pages::workspace::{shell::Browser, state::LaunchOptions},
        shared::i18n,
    };

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
            QuickAccess(&browser).add_quick_access_path(dir.clone());
        }
        Navigator(&browser).new_tab(Some(Location::from(work.as_path())));
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
