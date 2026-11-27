use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gds::app::Gds;
use gds::appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH};
use gds::i18n::Language;
use gds::service::task_manager::{Scope, Supervisor};
use zgui::app::Fonts;
use zgui::geom::{CssPx, DevicePx, Point, Size};
use zgui::platform::{AppHandler, PlatformError, SurfaceEvent};
use zgui::prelude::GetUntracked;
use zgui::vocab::{
    KeyEvent, Modifiers, PointerAction, PointerButton, PointerEvent, Role, Timestamp,
};
use zgui_platform_headless::Harness;

use super::super::list::{column_width, column_width_untracked};
use super::{
    assert_content_width_matches_layout, assert_directory_header_and_row_cell_parity,
    clear_view_probes, header_resize_node, last_color_sprites, noop_renderer, row_cell_node,
    send_ctrl_b, send_key_chord,
};
use crate::entities::file::columns::Column;
use crate::entities::location::{Location, places::Places, volumes};
use crate::entities::preferences;
use crate::pages::workspace::{
    navigation::Navigator,
    panes::Inspection,
    projection::Geometry,
    shell::{Browser, keyboard::Keyboard},
    state::LaunchOptions,
};
use crate::shared::i18n as files_i18n;

#[test]
fn column_header_resize_drag_tracks_continuous_pointer_moves_without_losing_capture() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(home.join("Desktop")).expect("create Desktop");
    std::fs::write(home.join("notes.txt"), b"hello\n").expect("create notes.txt");
    clear_view_probes();

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
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
        let deadline = Instant::now() + Duration::from_secs(3);
        while browser.read_untracked(|state| state.tab().rows.is_empty()) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for Home directory rows to load"
            );
            harness.advance(Duration::from_millis(25));
            harness.pump();
            std::thread::sleep(Duration::from_millis(10));
        }
        harness.settle(8);

        let initial = column_width_untracked(&browser, Column::Name);
        let initial_name_cell_0 = row_cell_node(0, Column::Name)
            .and_then(|n| n.get_untracked())
            .expect("bound row 0 Column::Name cell NodeId");
        assert_directory_header_and_row_cell_parity(&browser, &[0, 1]);
        let resize_node = header_resize_node(Column::Name)
            .expect("recorded .gds-files-header-resize for Column::Name");
        let initial_node_id = resize_node
            .get_untracked()
            .expect("bound resize handle NodeId");
        let scale = resize_node.scale().max(1.0);
        let bounds = resize_node
            .window_bounds()
            .expect("measured bounds for .gds-files-header-resize");
        let start_x = (bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale;
        let start_y = (bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale;

        // 1. PointerDown on `.gds-files-header-resize`, then multiple PointerMove steps (+40, +110, +180)
        // settled across frames to prove DOM nodes & pointer capture survive reactive width updates!
        let down_pos = Point::new(CssPx(start_x), CssPx(start_y));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(down_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);

        for delta in [40.0_f32, 110.0, 180.0] {
            let move_pos = Point::new(CssPx(start_x + delta), CssPx(start_y + 12.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(move_pos),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(6);

            let current = column_width(&browser, Column::Name);
            assert!(
                (current - (initial + delta)).abs() < 0.5,
                "expected Column::Name width == {:.1} after +{delta}px drag, got {current:.1}",
                initial + delta
            );
            let latest_resize = header_resize_node(Column::Name)
                .and_then(|n| n.get_untracked())
                .expect("resize handle still live");
            assert_eq!(
                latest_resize, initial_node_id,
                "header resize DOM node must not be destroyed/recreated during column drag"
            );
            assert_eq!(
                row_cell_node(0, Column::Name).and_then(|n| n.get_untracked()),
                Some(initial_name_cell_0),
                "row cell DOM node must remain stable during column resize drag"
            );
            assert_directory_header_and_row_cell_parity(&browser, &[0, 1]);
        }

        let up_pos = Point::new(CssPx(start_x + 180.0), CssPx(start_y + 12.0));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(up_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(6);
        assert!(
            (column_width(&browser, Column::Name) - (initial + 180.0)).abs() < 0.5,
            "expected final Column::Name width == {:.1} on PointerUp",
            initial + 180.0
        );

        // Moving after PointerUp must NOT continue resizing.
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(Point::new(CssPx(start_x + 250.0), CssPx(start_y))),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        assert!(
            (column_width(&browser, Column::Name) - (initial + 180.0)).abs() < 0.5,
            "pointer capture must release on PointerUp"
        );

        // Starting a second drag and firing PointerCancel must release capture and restore width.
        let resized_bounds = resize_node
            .window_bounds()
            .expect("updated bounds for .gds-files-header-resize");
        let second_x = (resized_bounds.origin.x.0 + resized_bounds.size.width.0 * 0.5) / scale;
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(Point::new(CssPx(second_x), CssPx(start_y)))
                .with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(Point::new(CssPx(second_x + 50.0), CssPx(start_y))),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        assert!(
            (column_width(&browser, Column::Name) - (initial + 230.0)).abs() < 0.5,
            "expected second drag to reach {:.1} before cancel",
            initial + 230.0
        );
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Cancelled,
            event: PointerEvent::mouse(Point::new(CssPx(second_x + 50.0), CssPx(start_y))),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(4);
        assert!(
            (column_width(&browser, Column::Name) - (initial + 180.0)).abs() < 0.5,
            "PointerCancel must restore pre-drag width ({:.1})",
            initial + 180.0
        );
        assert_directory_header_and_row_cell_parity(&browser, &[0, 1]);

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope: Scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::Chinese).expect("Chinese catalog"));
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
            browser.view()
        })
        .expect("column header resize harness");
}

#[test]
fn grid_tile_thumbnails_fill_full_tile_artwork_slot_and_details_media_spans_full_width_by_168px() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(home.join("Pictures")).expect("create Pictures");
    std::fs::write(home.join("Pictures/nested.bin"), vec![0xabu8; 4096])
        .expect("create nested file inside Pictures");
    std::fs::write(home.join("notes.txt"), b"hello\n").expect("create notes.txt");
    let two_line_name = "eb72d0c7423da74ca3d41fb9e570ce33470697977.txt";
    let very_long_name =
        "release_artifact_2026_11_21_x86_64_unknown_linux_gnu_symbols_bundle.tar.zst";
    std::fs::write(home.join(two_line_name), b"two lines\n").expect("create 2-line fixture");
    std::fs::write(home.join(very_long_name), b"long\n").expect("create 3-line fixture");
    let png_path = home.join("screenshot.png");
    let img: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> =
        image::ImageBuffer::from_fn(320, 180, |x, y| {
            image::Rgba([(x & 0xff) as u8, (y & 0xff) as u8, 200, 255])
        });
    img.save(&png_path).expect("write 16:9 PNG fixture");
    let png_len = std::fs::metadata(&png_path).expect("png metadata").len();
    let expected_multi_size =
        crate::entities::file::ui::EntryPresentation::format_size(png_len + 5 + 4096);

    clear_view_probes();
    crate::pages::workspace::panes::inspection::tests::clear_recorded_artwork_nodes();

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let start = home;

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        let offscreen = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("primary offscreen");
        offscreen.set_a11y_listener(true);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        let deadline = Instant::now() + Duration::from_secs(3);
        while browser.read_untracked(|state| state.tab().rows.len() < 5) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for Home directory rows to load"
            );
            harness.advance(Duration::from_millis(25));
            harness.pump();
            std::thread::sleep(Duration::from_millis(10));
        }

        // Switch to Grid view and open the Details pane.
        Navigator(&browser).toggle_view();
        Inspection(&browser).toggle_details();
        zgui::reactive::flush();
        harness.settle(16);

        let (screenshot_idx, notes_idx, pictures_idx, two_line_idx, very_long_idx) = browser
            .read_untracked(|state| {
                let tab = state.tab();
                let mut s_idx = None;
                let mut n_idx = None;
                let mut p_idx = None;
                let mut t_idx = None;
                let mut l_idx = None;
                for (idx, row) in tab.rows.iter().enumerate() {
                    match row.label().as_str() {
                        "screenshot.png" => s_idx = Some(idx),
                        "notes.txt" => n_idx = Some(idx),
                        "Pictures" => p_idx = Some(idx),
                        name if name == two_line_name => t_idx = Some(idx),
                        name if name == very_long_name => l_idx = Some(idx),
                        _ => {}
                    }
                }
                (
                    s_idx.expect("screenshot.png row index"),
                    n_idx.expect("notes.txt row index"),
                    p_idx.expect("Pictures row index"),
                    t_idx.expect("two-line row index"),
                    l_idx.expect("very-long row index"),
                )
            });

        // Select screenshot.png and wait for background inspection worker to decode its PNG thumbnail.
        Keyboard(&browser).select_row(screenshot_idx, Modifiers::NONE);
        zgui::reactive::flush();
        harness.settle(8);

        let cache = browser.0.inspections.get_untracked();
        let thumb_deadline = Instant::now() + Duration::from_secs(5);
        while cache.pending_count() > 0
            || cache.running_count() > 0
            || last_color_sprites().len() < 2
        {
            assert!(
                Instant::now() < thumb_deadline,
                "timed out waiting for screenshot.png thumbnail decode"
            );
            harness.advance(Duration::from_millis(20));
            harness.pump();
            zgui::reactive::flush();
            harness.settle(4);
            std::thread::sleep(Duration::from_millis(5));
        }
        zgui::reactive::flush();
        harness.settle(16);

        // 1. Grid tile thumbnail wrapper fills the full tile content width (> 120px) x 100px height!
        let tile_thumb =
            crate::pages::workspace::panes::inspection::tests::tile_thumb_node("screenshot.png")
                .expect("recorded .gds-files-tile-thumb-wrap for screenshot.png");
        let scale = tile_thumb.scale().max(1.0);
        let tile_thumb_bounds = tile_thumb
            .bounds()
            .expect("measured bounds for .gds-files-tile-thumb-wrap");
        let tile_thumb_w = tile_thumb_bounds.size.width.0 / scale;
        let tile_thumb_h = tile_thumb_bounds.size.height.0 / scale;
        assert!(
            tile_thumb_w > 120.0 && (tile_thumb_h - 100.0).abs() < 0.5,
            "expected Grid thumbnail wrapper > 120px wide x 100px tall (never 48px), got {tile_thumb_w:.1}x{tile_thumb_h:.1}"
        );

        // Verify actual painted ColorSprites for our 16:9 PNG fill full Grid (> 120px wide)
        // and Details (> 240px wide) slots via object-fit: contain (never 48x27px or 144x81px)!
        let sprites: Vec<(f32, f32)> = last_color_sprites()
            .into_iter()
            .map(|(w, h)| (w / scale, h / scale))
            .collect();
        assert!(
            sprites.iter().any(|&(w, h)| w > 120.0 && h > 65.0),
            "expected painted Grid 16:9 thumbnail ColorSprite > 120px wide (never 48x27px), got {sprites:?}"
        );
        assert!(
            sprites.iter().any(|&(w, h)| w > 240.0 && h > 130.0),
            "expected painted Details 16:9 thumbnail ColorSprite > 240px wide (never 144x81px), got {sprites:?}"
        );

        // 2. Fallback icons in Grid view remain 48x48px!
        for fallback_name in ["Pictures", "notes.txt"] {
            let fb_node = crate::pages::workspace::panes::inspection::tests::tile_fallback_node(
                fallback_name,
            )
            .unwrap_or_else(|| panic!("recorded fallback slot for {fallback_name}"));
            let fb_bounds = fb_node
                .bounds()
                .unwrap_or_else(|| panic!("measured bounds for {fallback_name} fallback"));
            let fb_w = fb_bounds.size.width.0 / scale;
            let fb_h = fb_bounds.size.height.0 / scale;
            assert!(
                (fb_w - 48.0).abs() < 0.5 && (fb_h - 48.0).abs() < 0.5,
                "expected Grid fallback icon for {fallback_name} to remain 48x48px, got {fb_w:.1}x{fb_h:.1}"
            );
        }

        // 3. No Shock contract: Details pane stage (168px), 2-line title slot (44px), and
        //    scrollable body origin Y stay at identical pixel coordinates across images,
        //    plain files, folders, 2-line filenames, 3+ line clamped filenames, and multi-select!
        let measure_details_anchors = |label: &str| -> (f32, f32, f32, f32, f32) {
            let stage = crate::pages::workspace::panes::inspection::tests::details_thumb_node()
                .unwrap_or_else(|| panic!("recorded .gds-files-details__thumb-wrap for {label}"));
            let title =
                crate::pages::workspace::panes::inspection::tests::details_title_slot_node()
                    .unwrap_or_else(|| {
                        panic!("recorded .gds-files-details__title-slot for {label}")
                    });
            let body = crate::pages::workspace::panes::inspection::tests::details_body_node()
                .unwrap_or_else(|| panic!("recorded .gds-files-details__body for {label}"));
            let sb = stage.window_bounds().expect("stage window bounds");
            let tb = title.window_bounds().expect("title slot window bounds");
            let bb = body.window_bounds().expect("body window bounds");
            (
                sb.size.width.0 / scale,
                sb.size.height.0 / scale,
                sb.origin.y.0 / scale,
                tb.origin.y.0 / scale,
                bb.origin.y.0 / scale,
            )
        };

        let (details_w, details_h, base_stage_y, base_title_y, base_body_y) =
            measure_details_anchors("screenshot.png");
        let base_title_h =
            crate::pages::workspace::panes::inspection::tests::details_title_slot_node()
                .expect("title slot")
                .bounds()
                .expect("title bounds")
                .size
                .height
                .0
                / scale;
        assert_content_width_matches_layout(&browser);
        assert!(
            details_w > 240.0 && (details_h - 168.0).abs() < 0.5,
            "expected Details media box to span full width (> 240px) x 168px, got {details_w:.1}x{details_h:.1}"
        );
        assert!(
            (base_title_h - 44.0).abs() < 0.5,
            "expected fixed 2-line Details title slot height == 44px, got {base_title_h:.1}"
        );

        // Selecting non-thumbnail `notes.txt` keeps the exact 168px stage + 44px title slot +
        // body Y anchor while centering a 64x64px icon inside the stage.
        Keyboard(&browser).select_row(notes_idx, Modifiers::NONE);
        zgui::reactive::flush();
        harness.settle(16);
        let (nw, nh, n_stage_y, n_title_y, n_body_y) = measure_details_anchors("notes.txt");
        assert!(
            nw > 240.0
                && (nh - 168.0).abs() < 0.5
                && (n_stage_y - base_stage_y).abs() < 0.5
                && (n_title_y - base_title_y).abs() < 0.5
                && (n_body_y - base_body_y).abs() < 0.5,
            "No Shock violation on notes.txt: stage=({nw:.1}x{nh:.1} @ {n_stage_y:.1}), title_y={n_title_y:.1}, body_y={n_body_y:.1} vs base ({base_stage_y:.1}, {base_title_y:.1}, {base_body_y:.1})"
        );
        let details_fb = crate::pages::workspace::panes::inspection::tests::details_fallback_node()
            .expect("recorded Details 64px fallback artwork");
        let details_fb_bounds = details_fb
            .bounds()
            .expect("measured bounds for Details 64px fallback");
        let dfb_w = details_fb_bounds.size.width.0 / scale;
        let dfb_h = details_fb_bounds.size.height.0 / scale;
        assert!(
            (dfb_w - 64.0).abs() < 0.5 && (dfb_h - 64.0).abs() < 0.5,
            "expected Details fallback icon inside 168px stage to measure 64x64px, got {dfb_w:.1}x{dfb_h:.1}"
        );

        // Check folder, 2-line hash filename, and 3+ line clamped filename: 0px anchor shift!
        for (idx, label) in [
            (pictures_idx, "Pictures"),
            (two_line_idx, two_line_name),
            (very_long_idx, very_long_name),
        ] {
            Keyboard(&browser).select_row(idx, Modifiers::NONE);
            zgui::reactive::flush();
            harness.settle(16);
            let (sw, sh, sy, ty, by) = measure_details_anchors(label);
            assert!(
                sw > 240.0
                    && (sh - 168.0).abs() < 0.5
                    && (sy - base_stage_y).abs() < 0.5
                    && (ty - base_title_y).abs() < 0.5
                    && (by - base_body_y).abs() < 0.5,
                "No Shock violation on {label}: stage=({sw:.1}x{sh:.1} @ {sy:.1}), title_y={ty:.1}, body_y={by:.1} vs base ({base_stage_y:.1}, {base_title_y:.1}, {base_body_y:.1})"
            );
        }

        // Check multi-selection (`Ctrl` toggle across 2 files + 1 folder): 0px anchor shift
        // AND displays item count ("项目数量"), folder/file breakdown, and combined byte size!
        Keyboard(&browser).select_row(screenshot_idx, Modifiers::CONTROL);
        Keyboard(&browser).select_row(pictures_idx, Modifiers::CONTROL);
        let multi_deadline = Instant::now() + Duration::from_secs(3);
        let live_texts = loop {
            harness.advance(Duration::from_millis(15));
            harness.pump();
            zgui::reactive::flush();
            harness.settle(8);
            let mut nodes = std::collections::HashMap::new();
            for update in offscreen.a11y_log() {
                for (id, node) in update.nodes {
                    nodes.insert(id, node);
                }
            }
            let texts: Vec<String> = nodes
                .values()
                .filter(|n| !n.is_hidden())
                .filter_map(|n| n.value().or_else(|| n.label()).map(str::to_owned))
                .collect();
            if texts.iter().any(|t| t == &expected_multi_size) {
                break texts;
            }
            assert!(
                Instant::now() < multi_deadline,
                "timed out waiting for multi-select combined size {expected_multi_size:?}; live_texts={texts:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        let items_label = gds::i18n::text("details-items");
        let size_label = gds::i18n::text("size");
        let folders_label = gds::i18n::text("details-folders");
        let files_label = gds::i18n::text("details-files");
        for expected_token in [
            items_label.as_str(),
            "3",
            size_label.as_str(),
            expected_multi_size.as_str(),
            folders_label.as_str(),
            files_label.as_str(),
        ] {
            assert!(
                live_texts.iter().any(|t| t == expected_token),
                "expected multi-select details row token {expected_token:?} in {live_texts:?}"
            );
        }
        let (mw, mh, my, mty, mby) = measure_details_anchors("multi-select");
        assert!(
            mw > 240.0
                && (mh - 168.0).abs() < 0.5
                && (my - base_stage_y).abs() < 0.5
                && (mty - base_title_y).abs() < 0.5
                && (mby - base_body_y).abs() < 0.5,
            "No Shock violation on multi-select: stage=({mw:.1}x{mh:.1} @ {my:.1}), title_y={mty:.1}, body_y={mby:.1} vs base ({base_stage_y:.1}, {base_title_y:.1}, {base_body_y:.1})"
        );

        // Re-selecting `screenshot.png` after toggle cycle keeps the full-width (> 240px) x 168px
        // media box and identical No Shock anchors.
        Keyboard(&browser).select_row(screenshot_idx, Modifiers::NONE);
        zgui::reactive::flush();
        harness.settle(16);
        assert!(
            !details_fb.is_bound(),
            "re-selecting image in Details pane must unmount the 64px fallback slot"
        );
        let (details_w2, details_h2, sy2, ty2, by2) =
            measure_details_anchors("screenshot.png re-select");
        assert!(
            details_w2 > 240.0
                && (details_h2 - 168.0).abs() < 0.5
                && (sy2 - base_stage_y).abs() < 0.5
                && (ty2 - base_title_y).abs() < 0.5
                && (by2 - base_body_y).abs() < 0.5,
            "expected re-selected Details media box to preserve full width x 168px and anchors, got {details_w2:.1}x{details_h2:.1}"
        );

        Inspection(&browser).toggle_details();
        zgui::reactive::flush();
        harness.settle(8);
        Inspection(&browser).toggle_details();
        zgui::reactive::flush();
        harness.settle(8);
        Keyboard(&browser).select_row(screenshot_idx, Modifiers::NONE);
        zgui::reactive::flush();
        harness.settle(8);
        Keyboard(&browser).select_row(notes_idx, Modifiers::NONE);
        zgui::reactive::flush();
        harness.settle(8);
        Keyboard(&browser).select_row(screenshot_idx, Modifiers::NONE);
        zgui::reactive::flush();
        harness.settle(16);
        let (details_w3, details_h3, sy3, ty3, by3) =
            measure_details_anchors("screenshot.png post-toggle");
        assert!(
            details_w3 > 240.0
                && (details_h3 - 168.0).abs() < 0.5
                && (sy3 - base_stage_y).abs() < 0.5
                && (ty3 - base_title_y).abs() < 0.5
                && (by3 - base_body_y).abs() < 0.5,
            "expected Details media box after toggle + regular-file switch to stay > 240px x 168px at fixed anchors, got {details_w3:.1}x{details_h3:.1}"
        );

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope: Scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::English).expect("English catalog"));
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
            browser.view()
        })
        .expect("grid thumbnail and details media layout harness");
}

#[test]
fn fresh_launch_ctrl_b_toggles_sidebar_repeatedly_across_frame_zero_transitions_and_sidebar_focus()
{
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(home.join("Desktop")).expect("create Desktop");
    std::fs::write(home.join("notes.txt"), b"hello\n").expect("create notes.txt");
    clear_view_probes();
    crate::pages::workspace::shell::chrome::tests::clear_recorded_tab_nodes();

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let start = home;

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        let browser = browser_slot.borrow().clone().expect("browser constructed");
        assert!(
            browser.0.sidebar.get_untracked(),
            "sidebar starts expanded at desktop width"
        );

        // 1. Frame 0 immediately after mounting `browser.view()`, BEFORE `settle()` / frame callbacks!
        for (step, expected) in [false, true, false, true].into_iter().enumerate() {
            send_ctrl_b(&mut harness);
            assert_eq!(
                browser.0.sidebar.get_untracked(),
                expected,
                "Frame-0 Ctrl+B press #{} expected sidebar == {expected}",
                step + 1
            );
        }

        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(8);
        let load_deadline = Instant::now() + Duration::from_secs(3);
        while browser.read_untracked(|state| state.tab().rows.len() < 2) {
            assert!(
                Instant::now() < load_deadline,
                "timed out waiting for directory rows before Ctrl+B transition"
            );
            harness.advance(Duration::from_millis(25));
            harness.pump();
            std::thread::sleep(Duration::from_millis(10));
        }
        harness.settle(8);

        // 2. Mid-transition (`48ms`) repeated `Ctrl+B` presses.
        for (step, expected) in [false, true, false, true].into_iter().enumerate() {
            send_ctrl_b(&mut harness);
            harness.run_for(Duration::from_millis(48), Duration::from_millis(16));
            assert_content_width_matches_layout(&browser);
            assert_directory_header_and_row_cell_parity(&browser, &[0, 1]);
            zgui::reactive::flush();
            assert_eq!(
                browser.0.sidebar.get_untracked(),
                expected,
                "Mid-transition (48ms) Ctrl+B press #{} expected sidebar == {expected}",
                step + 1
            );
        }
        harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
        harness.settle(4);

        // 3. Full `200ms` settle repeated `Ctrl+B` presses.
        for (step, expected) in [false, true, false, true].into_iter().enumerate() {
            send_ctrl_b(&mut harness);
            harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
            zgui::reactive::flush();
            harness.settle(4);
            assert_eq!(
                browser.0.sidebar.get_untracked(),
                expected,
                "Settled (200ms) Ctrl+B press #{} expected sidebar == {expected}",
                step + 1
            );
            let expected_w = if expected {
                SIDEBAR_WIDTH
            } else {
                COLLAPSED_SIDEBAR_WIDTH
            };
            assert!(
                (browser.sidebar_width().get_untracked() - expected_w).abs() < 0.5,
                "expected settled sidebar_width == {expected_w}, got {}",
                browser.sidebar_width().get_untracked()
            );
        }

        // 4. While a sidebar tab holds focus, and across unrelated revision / selection bumps!
        let tab_id = browser.read_untracked(|state| state.tab().id);
        let tab_node = crate::pages::workspace::shell::chrome::tests::recorded_tab_node(tab_id)
            .expect("recorded active sidebar tab node");
        let initial_tab_id = tab_node.get_untracked().expect("bound sidebar tab NodeId");
        browser.changed();
        browser.update_selection(|_| {});
        zgui::reactive::flush();
        harness.settle(4);
        assert_eq!(
            tab_node.get_untracked(),
            Some(initial_tab_id),
            "unrelated revision/selection bumps must never unmount or recreate sidebar_tab"
        );

        tab_node.focus();
        harness.settle(4);
        for (step, expected) in [false, true, false, true].into_iter().enumerate() {
            send_ctrl_b(&mut harness);
            harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
            zgui::reactive::flush();
            harness.settle(4);
            assert_eq!(
                browser.0.sidebar.get_untracked(),
                expected,
                "Tab-focused Ctrl+B press #{} expected sidebar == {expected}",
                step + 1
            );
            assert_eq!(
                tab_node.get_untracked(),
                Some(initial_tab_id),
                "sidebar_tab DOM node must remain stable across collapse/expand"
            );
        }

        // 5. While a sidebar heading (`Locations`) holds focus across `is_collapsed_rail`!
        let heading_node = crate::pages::workspace::shell::chrome::tests::heading_node("locations")
            .expect("recorded Locations sidebar heading node");
        let initial_heading_id = heading_node
            .get_untracked()
            .expect("bound sidebar heading NodeId");
        heading_node.focus();
        harness.settle(4);
        for (step, expected) in [false, true, false, true].into_iter().enumerate() {
            send_ctrl_b(&mut harness);
            harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
            zgui::reactive::flush();
            harness.settle(4);
            assert_eq!(
                browser.0.sidebar.get_untracked(),
                expected,
                "Heading-focused Ctrl+B press #{} expected sidebar == {expected}",
                step + 1
            );
            assert_eq!(
                heading_node.get_untracked(),
                Some(initial_heading_id),
                "sidebar_heading DOM node must remain stable across collapsed-rail tooltip gating"
            );
        }

        // 6. Micro window (< 600px) guards Ctrl+B so `sidebar` preference stays intact.
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(540.0),
            DevicePx(800.0),
        )));
        harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
        assert_eq!(
            Geometry(&browser).width_tier_untracked(),
            gds::ui::window::WidthTier::Micro
        );
        assert_content_width_matches_layout(&browser);
        send_ctrl_b(&mut harness);
        assert!(
            browser.0.sidebar.get_untracked(),
            "Ctrl+B in micro mode (< 600px) must not flip underlying sidebar state"
        );

        harness.deliver_to_first(SurfaceEvent::ScaleFactorChanged {
            scale_factor: 1.25,
            size: Size::new(DevicePx(960.0), DevicePx(800.0)),
        });
        harness.run_for(Duration::from_millis(200), Duration::from_millis(16));
        assert_content_width_matches_layout(&browser);
        assert_directory_header_and_row_cell_parity(&browser, &[0, 1]);

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope: Scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(true);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::Chinese).expect("Chinese catalog"));
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
            browser.view()
        })
        .expect("fresh-launch repeated Ctrl+B harness");
}

#[test]
fn item_properties_dialog_keeps_stable_height_across_tabs_and_recursive_toggle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let pictures = home.join("Pictures");
    let photos = home.join("Photos");
    std::fs::create_dir_all(&pictures).expect("create Pictures");

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let start = home;
    let photos_check = photos.clone();
    let pictures_check = pictures.clone();

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        let offscreen = harness
            .platform()
            .offscreens()
            .first()
            .cloned()
            .expect("primary offscreen");
        offscreen.set_a11y_listener(true);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        browser.edit_preferences(|prefs| prefs.tags.enabled = true);
        zgui::reactive::flush();
        harness.settle(8);

        let entry =
            crate::entities::file::directory::Entry::inspect(&pictures).expect("inspect Pictures");
        Inspection(&browser).edit_item_properties(entry, None);
        zgui::reactive::flush();
        for _ in 0..8 {
            harness.advance(Duration::from_millis(25));
            harness.pump();
            zgui::reactive::flush();
            harness.settle(4);
            std::thread::sleep(Duration::from_millis(10));
        }

        let find_bounds = |h: &Harness<Box<dyn AppHandler>>,
                           role: Role,
                           label: Option<&str>|
         -> Option<(f64, f64, f64, f64)> {
            let surface = h.platform().offscreens().first().cloned()?;
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
            let root_id = root?;
            let mut stack = vec![(root_id, 0.0_f64, 0.0_f64)];
            let mut seen = HashSet::new();
            while let Some((id, ox, oy)) = stack.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let Some(node) = nodes.get(&id) else {
                    continue;
                };
                let (nx, ny) = match node.transform() {
                    Some(t) => {
                        let c = t.as_coeffs();
                        (ox + c[4], oy + c[5])
                    }
                    None => (ox, oy),
                };
                let label_ok = label.is_none_or(|want| {
                    node.label() == Some(want)
                        || node.value() == Some(want)
                        || node.labelled_by().iter().any(|lid| {
                            nodes.get(lid).is_some_and(|labelled| {
                                labelled.label() == Some(want) || labelled.value() == Some(want)
                            })
                        })
                });
                if node.role() == role
                    && label_ok
                    && let Some(rect) = node.bounds()
                {
                    return Some((
                        nx + rect.x0,
                        ny + rect.y0,
                        rect.x1 - rect.x0,
                        rect.y1 - rect.y0,
                    ));
                }
                for &child in node.children().iter().rev() {
                    stack.push((child, nx, ny));
                }
            }
            None
        };

        let click_at = |h: &mut Harness<Box<dyn AppHandler>>, x: f64, y: f64, ms: u64| {
            let pos = Point::new(CssPx(x as f32), CssPx(y as f32));
            h.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
            });
            h.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::from_origin(Duration::from_millis(ms + 10)),
            });
            zgui::reactive::flush();
            h.settle(8);
        };

        let (_, _, base_w, base_h) = find_bounds(&harness, Role::Dialog, None)
            .expect("unified properties dialog bounds on General tab");

        // Close button uses IconButton-sized hit box (32x32).
        let (_, _, close_w, close_h) =
            find_bounds(&harness, Role::Button, Some("Close")).expect("dialog close button");
        assert!(
            close_w >= 30.0 && close_h >= 30.0,
            "dialog close button must have IconButton-sized hit box (~32x32), got {close_w:.1}x{close_h:.1}"
        );

        // Switch to Tags tab -> dialog geometry must remain identical.
        let (tx, ty, tw, th) =
            find_bounds(&harness, Role::Tab, Some("Tags")).expect("Tags tab trigger");
        click_at(&mut harness, tx + tw / 2.0, ty + th / 2.0, 50);
        let (_, _, tags_w, tags_h) =
            find_bounds(&harness, Role::Dialog, None).expect("dialog bounds on Tags tab");
        assert!(
            (tags_w - base_w).abs() < 0.5 && (tags_h - base_h).abs() < 0.5,
            "switching General -> Tags must not jump dialog size: base={base_w:.1}x{base_h:.1}, tags={tags_w:.1}x{tags_h:.1}"
        );

        // Switch to Permissions tab -> dialog geometry must remain identical.
        let (px, py, pw, ph) =
            find_bounds(&harness, Role::Tab, Some("Permissions")).expect("Permissions tab trigger");
        click_at(&mut harness, px + pw / 2.0, py + ph / 2.0, 100);
        let (_, _, perm_w, perm_h) =
            find_bounds(&harness, Role::Dialog, None).expect("dialog bounds on Permissions tab");
        assert!(
            (perm_w - base_w).abs() < 0.5 && (perm_h - base_h).abs() < 0.5,
            "switching Tags -> Permissions must not jump dialog size: base={base_w:.1}x{base_h:.1}, perm={perm_w:.1}x{perm_h:.1}"
        );

        // Toggle recursive checkbox inside Permissions -> dialog height must stay identical.
        let recursive_label = gds::i18n::text_untracked("permissions-recursive");
        let recursive_hint = gds::i18n::text_untracked("permissions-recursive-hint");
        assert!(
            find_bounds(
                &harness,
                Role::GenericContainer,
                Some(recursive_hint.as_str())
            )
            .is_none(),
            "recursive hint should not be visible before toggling"
        );
        let (rx, ry, rw, rh) =
            find_bounds(&harness, Role::CheckBox, Some(recursive_label.as_str()))
                .expect("recursive permissions checkbox");
        click_at(&mut harness, rx + rw / 2.0, ry + rh / 2.0, 200);
        assert!(
            find_bounds(
                &harness,
                Role::GenericContainer,
                Some(recursive_hint.as_str())
            )
            .is_some(),
            "clicking recursive checkbox should toggle it and reveal recursive hint"
        );
        let (_, _, rec_w, rec_h) = find_bounds(&harness, Role::Dialog, None)
            .expect("dialog bounds after toggling recursive checkbox");
        assert!(
            (rec_w - base_w).abs() < 0.5 && (rec_h - base_h).abs() < 0.5,
            "toggling recursive permissions must not jump dialog size: base={base_w:.1}x{base_h:.1}, rec={rec_w:.1}x{rec_h:.1}"
        );
        // Untoggle recursive so saving below only tests Basic rename + favorite cleanly.
        click_at(&mut harness, rx + rw / 2.0, ry + rh / 2.0, 250);

        // Switch to Comment tab -> dialog height must stay identical.
        let (cx, cy, cw, ch) =
            find_bounds(&harness, Role::Tab, Some("Comment")).expect("Comment tab trigger");
        click_at(&mut harness, cx + cw / 2.0, cy + ch / 2.0, 300);
        let (_, _, comment_w, comment_h) =
            find_bounds(&harness, Role::Dialog, None).expect("dialog bounds on Comment tab");
        assert!(
            (comment_w - base_w).abs() < 0.5 && (comment_h - base_h).abs() < 0.5,
            "switching to Comment tab must not jump dialog size: base={base_w:.1}x{base_h:.1}, comment={comment_w:.1}x{comment_h:.1}"
        );

        // Switch back to General tab -> rename Pictures -> Photos + toggle Favorite -> Save.
        let (gx, gy, gw, gh) =
            find_bounds(&harness, Role::Tab, Some("General")).expect("General tab trigger");
        click_at(&mut harness, gx + gw / 2.0, gy + gh / 2.0, 350);
        let fav_label = gds::i18n::text_untracked("details-basic-favorite");
        let (fx, fy, fw, fh) = find_bounds(&harness, Role::CheckBox, Some(fav_label.as_str()))
            .expect("favorite checkbox in General tab");
        click_at(&mut harness, fx + fw / 2.0, fy + fh / 2.0, 400);

        let (nx, ny, nw, nh) =
            find_bounds(&harness, Role::TextInput, Some("Name")).expect("Name input");
        click_at(&mut harness, nx + nw / 2.0, ny + nh / 2.0, 450);
        send_key_chord(&mut harness, KeyEvent::character("a"), Modifiers::CONTROL);
        for ch in ["P", "h", "o", "t", "o", "s"] {
            send_key_chord(&mut harness, KeyEvent::character(ch), Modifiers::NONE);
        }
        zgui::reactive::flush();
        harness.settle(4);

        let (sx, sy, sw, sh) =
            find_bounds(&harness, Role::Button, Some("Save")).expect("Save button");
        click_at(&mut harness, sx + sw / 2.0, sy + sh / 2.0, 500);
        for _ in 0..12 {
            harness.advance(Duration::from_millis(25));
            harness.pump();
            zgui::reactive::flush();
            harness.settle(4);
            if photos_check.is_dir() && find_bounds(&harness, Role::Dialog, None).is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            photos_check.is_dir() && !pictures_check.exists(),
            "saving General tab rename must rename Pictures -> Photos on disk"
        );
        assert!(
            browser.read_untracked(|state| {
                state
                    .places
                    .is_favorite(Location::from(photos_check.as_path()))
            }),
            "saving General tab favorite toggle must star the renamed path"
        );

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope: Scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::English).expect("English catalog"));
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
            browser.view()
        })
        .expect("item properties dialog stability harness");
}
