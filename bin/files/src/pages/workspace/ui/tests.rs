use super::{
    Browser, Command, FilesExt as _, InputEvent, KeyDownEvent, Keystroke, LaunchOptions, Modifiers,
    configuration, directory, pages, px, size, sorting, tokens,
};
use gds::app::AppExt as _;
use gpui_kit::{
    AppContext, Entity, TestAppContext, Window, WindowHandle, component::Root, test::TestWindowExt,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

fn init_test_workspace(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds().tasks().init_test(false);
        let tasks = cx.gds_ro().tasks().scope();
        cx.gds().initialize(tasks, None);
        cx.gds().appearance().motion().set_enabled(true);
        let catalog = crate::shared::i18n::prepare(gds::i18n::Language::English)
            .expect("prepare English translations");
        cx.gds().i18n().install_catalog(catalog);
        let mut preferences = configuration::Preferences::default();
        preferences.appearance.source = gds::appearance::theme::Source::Builtin;
        cx.gds()
            .appearance()
            .theme()
            .apply(&preferences.appearance)
            .expect("apply default theme");
        cx.gds()
            .appearance()
            .typography()
            .apply(&preferences.font)
            .expect("apply default typography");
        cx.gds()
            .files()
            .preferences()
            .initialize(preferences)
            .expect("initialize preferences");
        cx.gds().files().volumes().initialize();
        cx.gds().files().register_actions();
    });
}

fn open_test_browser(
    cx: &mut TestAppContext,
    home: PathBuf,
    places: Result<
        crate::entities::location::places::Places,
        crate::entities::location::places::Error,
    >,
) -> (WindowHandle<Root>, Entity<Browser>) {
    let mut browser = None;
    let window = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
        let view = cx.new(|cx| {
            Browser::new(
                LaunchOptions {
                    places,
                    start: home.clone(),
                    home,
                    picker: None,
                },
                window,
                cx,
            )
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    (window, browser.expect("browser entity created"))
}

fn wait_for_browser(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    browser: &Entity<Browser>,
    predicate: impl Fn(&Browser, &gpui_kit::App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        let ready = cx
            .update_window(window.into(), |_, _, cx| predicate(browser.read(cx), cx))
            .expect("poll browser condition");
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for browser condition"
        );
        std::thread::yield_now();
    }
}

fn set_filter_query(
    browser: &Entity<Browser>,
    query: &str,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let input = browser.read(cx).toolbar.filter.input.clone();
    input.update(cx, |input, cx| {
        input.set_value(query.to_owned(), window, cx)
    });
    browser.update(cx, |browser, cx| {
        browser.on_filter_input(&input, &InputEvent::Change, window, cx);
    });
    window.render_frame(cx);
}

fn visible_names(browser: &Entity<Browser>, cx: &gpui_kit::App) -> Vec<String> {
    browser
        .read(cx)
        .active()
        .expect("active directory tab")
        .table_rows
        .iter()
        .map(|entry| entry.display_name.clone())
        .collect()
}

fn set_entry_mtime(path: &Path, seconds_after_epoch: u64) {
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds_after_epoch);
    std::fs::File::open(path)
        .and_then(|file| file.set_modified(mtime))
        .expect("set entry mtime");
}

fn populate_large_directory(home: &Path) {
    (0..20).for_each(|index| {
        std::fs::create_dir_all(home.join(format!("folder-{index:04}")))
            .expect("create sample folder");
    });
    (20..1_000).for_each(|index| {
        std::fs::write(home.join(format!("file-{index:04}.txt")), b"payload\n")
            .expect("create sample file");
    });
}

fn dispatch_shift_down_batches(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    browser: &Entity<Browser>,
    shift_down: &KeyDownEvent,
) -> (Duration, Duration) {
    let start = Instant::now();
    let mut key_dispatch_elapsed = Duration::ZERO;
    (0..24).for_each(|batch| {
        cx.update_window(window.into(), |_, window, cx| {
            let batch_start = Instant::now();
            (0..25).for_each(|_| {
                browser.update(cx, |browser, cx| {
                    browser.on_file_key_down(shift_down, window, cx);
                });
            });
            key_dispatch_elapsed += batch_start.elapsed();
            window.render_frame(cx);
        })
        .expect("dispatch Shift+Down batch");
        if batch % 2 == 1 {
            cx.run_until_parked();
        }
    });
    cx.run_until_parked();
    (key_dispatch_elapsed, start.elapsed())
}

#[gpui_kit::test]
async fn startup_sidebar_toggle_settings_and_resize_animate_in_render(cx: &mut TestAppContext) {
    init_test_workspace(cx);
    let directory = tempfile::tempdir().expect("create temporary workspace directory");
    let home = directory.path().join("home");
    std::fs::create_dir_all(&home).expect("create home directory");
    std::fs::write(home.join("sample.txt"), b"hello\n").expect("create sample file");

    let (window, browser) = open_test_browser(
        cx,
        home,
        Err(crate::entities::location::places::Error::Remote(
            "corrupt places".into(),
        )),
    );
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let view = browser.read(cx);
        assert!(view.locations.load_error.is_some());
        assert!(!view.window.layout.compact);
        assert_eq!(view.sidebar.presentation.width(), tokens::SIDEBAR_WIDTH);
        assert_eq!(view.sidebar.presentation.progress(), 1.0);

        browser.update(cx, |browser, cx| {
            browser.dispatch(Command::Sidebar, window, cx);
        });
        window.render_frame(cx);
        assert!(browser.read(cx).sidebar.collapsed);
    })
    .expect("toggle sidebar to collapsed");

    cx.executor().advance_clock(Duration::from_millis(60));
    cx.update_window(window.into(), |_, window, cx| {
        assert!(window.simulate_next_frame(cx) >= 1);
    })
    .expect("advance sidebar collapse midpoint");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, _, cx| {
        let presentation = browser.read(cx).sidebar.presentation;
        assert!(presentation.progress() > 0.0 && presentation.progress() < 1.0);
        assert!(
            presentation.width() > tokens::COLLAPSED_SIDEBAR_WIDTH
                && presentation.width() < tokens::SIDEBAR_WIDTH
        );
    })
    .expect("verify intermediate sidebar presentation");

    cx.executor().advance_clock(Duration::from_millis(120));
    cx.update_window(window.into(), |_, window, cx| {
        window.simulate_next_frame(cx);
    })
    .expect("complete sidebar collapse");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        let presentation = browser.read(cx).sidebar.presentation;
        assert_eq!(presentation.width(), tokens::COLLAPSED_SIDEBAR_WIDTH);
        assert_eq!(presentation.progress(), 0.0);

        browser.update(cx, |browser, cx| {
            browser.dispatch(Command::Settings, window, cx);
        });
        window.render_frame(cx);
        assert!(browser.read(cx).pages.current() == Some(&pages::Page::Settings));

        browser.update(cx, |browser, cx| {
            browser.dispatch(Command::Cancel, window, cx);
            browser.dispatch(Command::Sidebar, window, cx);
        });
        window.render_frame(cx);
        assert!(browser.read(cx).pages.is_empty());
        assert!(!browser.read(cx).sidebar.collapsed);
    })
    .expect("open settings and expand sidebar");

    cx.executor().advance_clock(Duration::from_millis(180));
    cx.update_window(window.into(), |_, window, cx| {
        window.simulate_next_frame(cx);
    })
    .expect("complete sidebar expand");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        assert_eq!(
            browser.read(cx).sidebar.presentation.width(),
            tokens::SIDEBAR_WIDTH
        );
        window.resize(size(px(900.), px(700.)));
        window.bounds_changed(cx);
        window.render_frame(cx);
        assert!(browser.read(cx).window.layout.compact);
    })
    .expect("resize below compact threshold");

    cx.executor().advance_clock(Duration::from_millis(180));
    cx.update_window(window.into(), |_, window, cx| {
        window.simulate_next_frame(cx);
    })
    .expect("settle compact sidebar transition");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        assert_eq!(
            browser.read(cx).sidebar.presentation.width(),
            tokens::COLLAPSED_SIDEBAR_WIDTH
        );
        window.resize(size(px(1280.), px(800.)));
        window.bounds_changed(cx);
        window.render_frame(cx);
        assert!(!browser.read(cx).window.layout.compact);
    })
    .expect("resize above compact threshold");

    let scope = cx.update(|cx| cx.gds_ro().tasks().scope());
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    scope.shutdown().await;
}

#[gpui_kit::test]
async fn shift_down_600_rows_in_large_directory_scales_linearly_and_preserves_selection(
    cx: &mut TestAppContext,
) {
    init_test_workspace(cx);
    let directory = tempfile::tempdir().expect("create temporary workspace directory");
    let home = directory.path().join("home");
    std::fs::create_dir_all(&home).expect("create home directory");
    populate_large_directory(&home);

    let (window, browser) = open_test_browser(
        cx,
        home,
        Ok(crate::entities::location::places::Places::default()),
    );
    wait_for_browser(cx, window, &browser, |view, _| {
        view.active()
            .is_some_and(|slot| !slot.is_loading() && slot.table_rows.len() == 1_000)
    });

    let expected_paths: Vec<PathBuf> = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            browser.update(cx, |browser, cx| {
                browser.window.focus.focus(window, cx);
                browser.select(0, Modifiers::default(), cx);
            });
            window.render_frame(cx);
            let view = browser.read(cx);
            let slot = view.active().expect("active directory tab");
            assert_eq!(slot.table_rows.len(), 1_000);
            assert_eq!(slot.navigation.view().selected.len(), 1);
            assert_eq!(slot.cursor_index, Some(0));
            assert_eq!(slot.anchor_index, Some(0));
            slot.table_rows
                .iter()
                .map(|entry| entry.path.clone())
                .collect()
        })
        .expect("initialize row 0 selection");
    cx.run_until_parked();

    let shift_down = KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            key: "down".into(),
            key_char: None,
        },
        is_held: true,
        prefer_character_input: false,
    };

    let (key_dispatch_elapsed, elapsed) =
        dispatch_shift_down_batches(cx, window, &browser, &shift_down);
    eprintln!(
        "600 consecutive Shift+Down keystrokes over 1,000 entries: key_dispatch={key_dispatch_elapsed:?}, total_with_24_frames={elapsed:?}"
    );
    assert!(
        key_dispatch_elapsed < Duration::from_millis(500),
        "600 Shift+Down state/table syncs took too long in debug build: {key_dispatch_elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "600 Shift+Down keystrokes + 24 frame renders took too long in debug build: {elapsed:?}"
    );

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let view = browser.read(cx);
        let slot = view.active().expect("active directory tab");
        let selected = &slot.navigation.view().selected;
        assert_eq!(selected.len(), 601);
        assert_eq!(selected.first(), Some(&expected_paths[0]));
        assert_eq!(selected.last(), Some(&expected_paths[600]));
        assert!(selected.contains(&expected_paths[0]));
        assert!(selected.contains(&expected_paths[256]));
        assert!(selected.contains(&expected_paths[600]));
        assert!(!selected.contains(&expected_paths[601]));
        assert_eq!(slot.anchor.as_ref(), Some(&expected_paths[0]));
        assert_eq!(slot.anchor_index, Some(0));
        assert_eq!(slot.cursor.as_ref(), Some(&expected_paths[600]));
        assert_eq!(slot.cursor_index, Some(600));
        let scroll_y = slot.scroll.0.borrow().base_handle.offset().y;
        assert!(
            scroll_y < px(0.),
            "expected scroll offset to advance past initial viewport, got {scroll_y:?}"
        );
    })
    .expect("verify 600 Shift+Down range selection");

    let shift_up = KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            key: "up".into(),
            key_char: None,
        },
        is_held: true,
        prefer_character_input: false,
    };
    cx.update_window(window.into(), |_, window, cx| {
        (0..10).for_each(|_| {
            browser.update(cx, |browser, cx| {
                browser.on_file_key_down(&shift_up, window, cx);
            });
        });
        window.render_frame(cx);
    })
    .expect("dispatch 10 Shift+Up steps");

    cx.update_window(window.into(), |_, window, cx| {
        let view = browser.read(cx);
        let slot = view.active().expect("active directory tab");
        let selected = &slot.navigation.view().selected;
        assert_eq!(selected.len(), 591);
        assert_eq!(selected.first(), Some(&expected_paths[0]));
        assert_eq!(selected.last(), Some(&expected_paths[590]));
        assert!(selected.contains(&expected_paths[590]));
        assert!(!selected.contains(&expected_paths[591]));
        assert_eq!(slot.anchor_index, Some(0));
        assert_eq!(slot.cursor_index, Some(590));

        // Ctrl-toggle an interior item transitions cleanly out of Range and updates anchor/cursor.
        browser.update(cx, |browser, cx| {
            browser.select(
                585,
                Modifiers {
                    control: true,
                    ..Default::default()
                },
                cx,
            );
        });
        window.render_frame(cx);
        let view = browser.read(cx);
        let slot = view.active().expect("active directory tab");
        let selected = &slot.navigation.view().selected;
        assert_eq!(selected.len(), 590);
        assert!(!selected.contains(&expected_paths[585]));
        assert!(selected.contains(&expected_paths[0]));
        assert!(selected.contains(&expected_paths[590]));
        assert_eq!(slot.anchor_index, Some(585));
        assert_eq!(slot.cursor_index, Some(585));

        // Plain Down collapses to a single-item selection at the next row.
        let plain_down = KeyDownEvent {
            keystroke: Keystroke {
                modifiers: Modifiers::default(),
                key: "down".into(),
                key_char: None,
            },
            is_held: false,
            prefer_character_input: false,
        };
        browser.update(cx, |browser, cx| {
            browser.on_file_key_down(&plain_down, window, cx);
        });
        window.render_frame(cx);
        let view = browser.read(cx);
        let slot = view.active().expect("active directory tab");
        let selected = &slot.navigation.view().selected;
        assert_eq!(selected.len(), 1);
        assert_eq!(selected.first(), Some(&expected_paths[586]));
        assert_eq!(slot.anchor_index, Some(586));
        assert_eq!(slot.cursor_index, Some(586));
    })
    .expect("verify Shift+Up shrink, Ctrl toggle, and plain Down collapse");

    let scope = cx.update(|cx| cx.gds_ro().tasks().scope());
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    scope.shutdown().await;
}

#[gpui_kit::test]
async fn non_canonical_sort_filter_reuses_projection_preserves_order_and_prunes_hidden_selection(
    cx: &mut TestAppContext,
) {
    init_test_workspace(cx);
    let temp = tempfile::tempdir().expect("create temporary workspace directory");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("create home directory");

    let dir_alpha = home.join("dir-alpha");
    std::fs::create_dir(&dir_alpha).expect("create dir-alpha");
    std::fs::write(dir_alpha.join("one.txt"), b"1").expect("write dir-alpha child");
    set_entry_mtime(&dir_alpha, 100);

    let dir_omega = home.join("dir-omega");
    std::fs::create_dir(&dir_omega).expect("create dir-omega");
    std::fs::write(dir_omega.join("one.txt"), b"1").expect("write dir-omega child 1");
    std::fs::write(dir_omega.join("two.txt"), b"2").expect("write dir-omega child 2");
    std::fs::write(dir_omega.join("three.txt"), b"3").expect("write dir-omega child 3");
    set_entry_mtime(&dir_omega, 300);

    let note_alpha = home.join("note-alpha.txt");
    std::fs::write(&note_alpha, vec![b'a'; 10]).expect("write note-alpha");
    set_entry_mtime(&note_alpha, 1_000);

    let note_beta = home.join("note-beta.txt");
    std::fs::write(&note_beta, vec![b'b'; 300]).expect("write note-beta");
    set_entry_mtime(&note_beta, 3_000);

    let note_gamma = home.join("note-gamma.txt");
    std::fs::write(&note_gamma, vec![b'g'; 50]).expect("write note-gamma");
    set_entry_mtime(&note_gamma, 2_000);

    let report_delta = home.join("report-delta.md");
    std::fs::write(&report_delta, vec![b'd'; 500]).expect("write report-delta");
    set_entry_mtime(&report_delta, 4_000);

    let (window, browser) = open_test_browser(
        cx,
        home.clone(),
        Ok(crate::entities::location::places::Places::default()),
    );
    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            !slot.is_loading()
                && slot.table_rows.len() == 6
                && slot.local_enrich().is_some_and(|job| !job.is_in_flight())
        })
    });

    // 1. Name Descending sort: cache projection, filter typing/narrowing/backspacing/clearing, and prune hidden selection.
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.window.focus.focus(window, cx);
            browser.set_sort(
                sorting::Sort {
                    key: sorting::Key::Name,
                    direction: sorting::Direction::Descending,
                },
                cx,
            );
            browser.select_all_files(window, cx);
            browser.select(
                2,
                Modifiers {
                    control: false,
                    ..Default::default()
                },
                cx,
            );
            browser.select_all_files(window, cx);
        });
        window.render_frame(cx);

        assert_eq!(
            visible_names(&browser, cx),
            [
                "dir-omega",
                "dir-alpha",
                "report-delta.md",
                "note-gamma.txt",
                "note-beta.txt",
                "note-alpha.txt"
            ]
        );
        let slot = browser.read(cx).active().expect("active tab");
        let cached_name_desc = Arc::clone(
            &slot
                .sorted_projection
                .as_ref()
                .expect("cached Name desc projection")
                .entries,
        );
        assert!(Arc::ptr_eq(&slot.table_rows, &cached_name_desc));
        assert_eq!(slot.navigation.view().selected.len(), 6);

        // Type "note": preserves descending order, reuses cached projection, and prunes hidden selections.
        set_filter_query(&browser, "note", window, cx);
        assert_eq!(
            visible_names(&browser, cx),
            ["note-gamma.txt", "note-beta.txt", "note-alpha.txt"]
        );
        let slot = browser.read(cx).active().expect("active tab");
        assert!(Arc::ptr_eq(
            &slot
                .sorted_projection
                .as_ref()
                .expect("cached projection")
                .entries,
            &cached_name_desc
        ));
        assert_eq!(slot.navigation.view().selected.len(), 3);
        assert!(!slot.navigation.view().selected.contains(&report_delta));
        assert_eq!(
            browser.read(cx).selected_visible_paths(),
            vec![note_gamma.clone(), note_beta.clone(), note_alpha.clone()]
        );

        // Narrow to "note-b": only note-beta.txt remains selected and visible.
        set_filter_query(&browser, "note-b", window, cx);
        assert_eq!(visible_names(&browser, cx), ["note-beta.txt"]);
        let slot = browser.read(cx).active().expect("active tab");
        assert!(Arc::ptr_eq(
            &slot
                .sorted_projection
                .as_ref()
                .expect("cached projection")
                .entries,
            &cached_name_desc
        ));
        assert_eq!(
            slot.navigation.view().selected.to_vec(),
            vec![note_beta.clone()]
        );

        // Backspace to "note": order stays descending and previously pruned items stay deselected.
        set_filter_query(&browser, "note", window, cx);
        assert_eq!(
            visible_names(&browser, cx),
            ["note-gamma.txt", "note-beta.txt", "note-alpha.txt"]
        );
        let slot = browser.read(cx).active().expect("active tab");
        assert_eq!(
            slot.navigation.view().selected.to_vec(),
            vec![note_beta.clone()]
        );

        // Clear filter: O(1) Arc reuse of cached sorted_projection.
        set_filter_query(&browser, "", window, cx);
        let slot = browser.read(cx).active().expect("active tab");
        assert!(Arc::ptr_eq(&slot.table_rows, &cached_name_desc));
    })
    .expect("verify Name descending sort with search filter");

    // 2. Modified Descending sort.
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.set_sort(
                sorting::Sort {
                    key: sorting::Key::Modified,
                    direction: sorting::Direction::Descending,
                },
                cx,
            );
            browser.select_all_files(window, cx);
        });
        window.render_frame(cx);

        assert_eq!(
            visible_names(&browser, cx),
            [
                "dir-omega",
                "dir-alpha",
                "report-delta.md",
                "note-beta.txt",
                "note-gamma.txt",
                "note-alpha.txt"
            ]
        );
        let cached_modified = Arc::clone(
            &browser
                .read(cx)
                .active()
                .expect("active tab")
                .sorted_projection
                .as_ref()
                .expect("cached Modified projection")
                .entries,
        );

        set_filter_query(&browser, "note", window, cx);
        assert_eq!(
            visible_names(&browser, cx),
            ["note-beta.txt", "note-gamma.txt", "note-alpha.txt"]
        );
        set_filter_query(&browser, "gamma", window, cx);
        assert_eq!(visible_names(&browser, cx), ["note-gamma.txt"]);
        assert_eq!(
            browser.read(cx).selected_visible_paths(),
            vec![note_gamma.clone()]
        );

        set_filter_query(&browser, "note", window, cx);
        assert_eq!(
            visible_names(&browser, cx),
            ["note-beta.txt", "note-gamma.txt", "note-alpha.txt"]
        );
        set_filter_query(&browser, "", window, cx);
        let slot = browser.read(cx).active().expect("active tab");
        assert!(Arc::ptr_eq(&slot.table_rows, &cached_modified));
    })
    .expect("verify Modified descending sort with search filter");

    // 3. Size Descending sort.
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.set_sort(
                sorting::Sort {
                    key: sorting::Key::Size,
                    direction: sorting::Direction::Descending,
                },
                cx,
            );
            browser.select_all_files(window, cx);
        });
        window.render_frame(cx);

        assert_eq!(
            visible_names(&browser, cx),
            [
                "dir-omega",
                "dir-alpha",
                "report-delta.md",
                "note-beta.txt",
                "note-gamma.txt",
                "note-alpha.txt"
            ]
        );
        let cached_size = Arc::clone(
            &browser
                .read(cx)
                .active()
                .expect("active tab")
                .sorted_projection
                .as_ref()
                .expect("cached Size projection")
                .entries,
        );

        set_filter_query(&browser, "note", window, cx);
        assert_eq!(
            visible_names(&browser, cx),
            ["note-beta.txt", "note-gamma.txt", "note-alpha.txt"]
        );
        set_filter_query(&browser, "note-a", window, cx);
        assert_eq!(visible_names(&browser, cx), ["note-alpha.txt"]);
        assert_eq!(
            browser.read(cx).selected_visible_paths(),
            vec![note_alpha.clone()]
        );

        set_filter_query(&browser, "note", window, cx);
        assert_eq!(
            visible_names(&browser, cx),
            ["note-beta.txt", "note-gamma.txt", "note-alpha.txt"]
        );
        set_filter_query(&browser, "", window, cx);
        let slot = browser.read(cx).active().expect("active tab");
        assert!(Arc::ptr_eq(&slot.table_rows, &cached_size));
    })
    .expect("verify Size descending sort with search filter");

    let scope = cx.update(|cx| cx.gds_ro().tasks().scope());
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    scope.shutdown().await;
}

fn populate_enrichment_directories(home: &Path, other: &Path) {
    (0..300).for_each(|index| {
        let dir = home.join(format!("folder-{index:03}"));
        std::fs::create_dir_all(&dir).expect("create test folder");
    });
    std::fs::write(home.join("folder-000/a.txt"), b"1").expect("write folder-000 child");
    std::fs::write(home.join("folder-150/a.txt"), b"1").expect("write folder-150 child 1");
    std::fs::write(home.join("folder-150/b.txt"), b"2").expect("write folder-150 child 2");
    std::fs::write(home.join("folder-299/a.txt"), b"1").expect("write folder-299 child 1");
    std::fs::write(home.join("folder-299/b.txt"), b"2").expect("write folder-299 child 2");
    std::fs::write(home.join("folder-299/c.txt"), b"3").expect("write folder-299 child 3");

    std::fs::create_dir_all(other).expect("create other directory");
    std::fs::write(other.join("inside-other.txt"), b"other\n").expect("write other child");
}

#[gpui_kit::test]
async fn size_sort_enrichment_and_viewport_priority_settle_order_selection_and_revision(
    cx: &mut TestAppContext,
) {
    init_test_workspace(cx);
    let temp = tempfile::tempdir().expect("create temporary workspace directory");
    let home = temp.path().join("home");
    let other = temp.path().join("other");
    populate_enrichment_directories(&home, &other);

    let (window, browser) = open_test_browser(
        cx,
        home.clone(),
        Ok(crate::entities::location::places::Places::default()),
    );
    wait_for_browser(cx, window, &browser, |view, _| {
        view.active()
            .is_some_and(|slot| !slot.is_loading() && slot.table_rows.len() == 300)
    });

    // Promote tail viewport rows (beyond DEFAULT_PROBE_BUDGET = 256) while or after initial enrichment runs.
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            let tail = browser.active().expect("active tab").table_rows[280..300].to_vec();
            browser.request_viewport_enrichment(&tail, cx);
            browser.select(0, Modifiers::default(), cx);
            browser.select(
                299,
                Modifiers {
                    control: true,
                    ..Default::default()
                },
                cx,
            );
        });
        window.render_frame(cx);
    })
    .expect("request viewport enrichment for tail rows and select endpoints");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            slot.local_enrich().is_some_and(|job| !job.is_in_flight())
                && slot.table_rows[299].details.children
                    == Some(directory::ChildCount {
                        count: 3,
                        truncated: false,
                    })
        })
    });

    // Switch to Sort::Size Descending: triggers full-budget enrichment across all 300 directories
    // and settles with exact child-count ordering while retaining the selected paths.
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.set_sort(
                sorting::Sort {
                    key: sorting::Key::Size,
                    direction: sorting::Direction::Descending,
                },
                cx,
            );
        });
        window.render_frame(cx);
    })
    .expect("switch to Size descending sort");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            slot.local_enrich()
                .is_some_and(|job| !job.is_in_flight() && job.need().full_budget)
                && slot.table_rows[0].display_name == "folder-299"
                && slot.table_rows[1].display_name == "folder-150"
                && slot.table_rows[2].display_name == "folder-000"
        })
    });

    let home_revision = cx
        .update_window(window.into(), |_, _, cx| {
            let view = browser.read(cx);
            let slot = view.active().expect("active tab");
            assert_eq!(slot.navigation.view().selected.len(), 2);
            assert!(
                slot.navigation
                    .view()
                    .selected
                    .contains(&home.join("folder-000"))
            );
            assert!(
                slot.navigation
                    .view()
                    .selected
                    .contains(&home.join("folder-299"))
            );
            slot.navigation.revision()
        })
        .expect("verify settled Size sort order and selection retention");

    // Trigger refresh on home and immediately navigate to `other` to verify revision safety.
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.dispatch(Command::Refresh, window, cx);
            browser.navigate(other.clone(), cx);
        });
        window.render_frame(cx);
    })
    .expect("refresh and navigate to other directory");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            !slot.is_loading()
                && slot.navigation.path() == Some(other.as_path())
                && slot.table_rows.len() == 1
                && slot.local_enrich().is_some_and(|job| !job.is_in_flight())
        })
    });

    cx.update_window(window.into(), |_, _, cx| {
        let view = browser.read(cx);
        let slot = view.active().expect("active tab");
        assert!(slot.navigation.revision() > home_revision);
        assert_eq!(slot.table_rows.len(), 1);
        assert_eq!(slot.table_rows[0].path, other.join("inside-other.txt"));
    })
    .expect("verify revision safety after navigation");

    let scope = cx.update(|cx| cx.gds_ro().tasks().scope());
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    scope.shutdown().await;
}
