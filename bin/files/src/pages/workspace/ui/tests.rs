use super::{
    Browser, Command, InputEvent, KeyDownEvent, Keystroke, LaunchOptions, Location, Modifiers,
    PreferencesExt as _, ViewMode, VolumesExt as _, configuration, directory, pages, px, size,
    sorting, tokens,
};
use crate::entities::location::places::Places;
use gds::app::AppExt as _;
use gpui_kit::{
    AppContext, Entity, TestAppContext, Window, WindowHandle, component::Root, test::TestWindowExt,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

fn init_test_workspace(cx: &mut TestAppContext) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("create temporary workspace directory");
    let settings_path = temp.path().join("settings.toml");
    cx.executor().allow_parking();
    cx.update(|cx| {
        cx.gds_mut().tasks().init_test(false);
        let tasks = cx.gds().tasks().scope();
        cx.gds_mut().init_ui(None);
        cx.gds_mut().appearance().motion().set_enabled(true);
        let catalog = crate::shared::i18n::prepare(gds::i18n::Language::English)
            .expect("prepare English translations");
        cx.gds_mut().i18n().install_catalog(catalog);
        let mut preferences = configuration::Preferences::default();
        preferences.appearance.source = gds::appearance::theme::Source::Builtin;
        cx.gds_mut()
            .appearance()
            .theme()
            .apply(&preferences.appearance)
            .expect("apply default theme");
        cx.gds_mut()
            .appearance()
            .typography()
            .apply(&preferences.font)
            .expect("apply default typography");
        cx.gds_mut()
            .appearance()
            .theme()
            .init_hot_reload(preferences.appearance.clone());
        cx.gds_mut()
            .preferences()
            .init_test(preferences, settings_path, &tasks);
        cx.gds_mut().volumes().initialize(&tasks);
        Command::register(cx);
    });
    temp
}

fn open_test_browser(
    cx: &mut TestAppContext,
    home: PathBuf,
    places: Result<Places, crate::entities::location::places::Error>,
) -> (WindowHandle<Root>, Entity<Browser>) {
    open_test_browser_at(cx, home.clone(), home, places)
}

fn open_test_browser_at(
    cx: &mut TestAppContext,
    start: PathBuf,
    home: PathBuf,
    places: Result<Places, crate::entities::location::places::Error>,
) -> (WindowHandle<Root>, Entity<Browser>) {
    let mut browser = None;
    let window = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
        let view = cx.new(|cx| {
            Browser::new(
                LaunchOptions {
                    places,
                    start,
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

fn wait_for_tabs_loaded(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    browser: &Entity<Browser>,
) {
    wait_for_browser(cx, window, browser, |view, _| {
        !view.tabs.items.is_empty()
            && view.tabs.items.iter().all(|slot| {
                !slot.is_loading()
                    && (slot.path().is_none() || slot.navigation.directory().is_some())
            })
    });
}

async fn close_test_browser(cx: &mut TestAppContext, window: WindowHandle<Root>) {
    let scope = cx.update(|cx| cx.gds().tasks().scope());
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    scope.shutdown().await;
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
    let directory = init_test_workspace(cx);
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

    let scope = cx.update(|cx| cx.gds().tasks().scope());
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    scope.shutdown().await;
}

#[gpui_kit::test]
async fn shift_down_600_rows_in_large_directory_scales_linearly_and_preserves_selection(
    cx: &mut TestAppContext,
) {
    let directory = init_test_workspace(cx);
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

    let scope = cx.update(|cx| cx.gds().tasks().scope());
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .expect("remove test window");
    scope.shutdown().await;
}

#[gpui_kit::test]
async fn non_canonical_sort_filter_reuses_projection_preserves_order_and_prunes_hidden_selection(
    cx: &mut TestAppContext,
) {
    let temp = init_test_workspace(cx);
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

    let scope = cx.update(|cx| cx.gds().tasks().scope());
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
    let temp = init_test_workspace(cx);
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
                .is_some_and(|job| !job.is_in_flight() && job.need().is_full_budget())
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

    close_test_browser(cx, window).await;
}

#[gpui_kit::test]
async fn opening_window_with_pinned_tabs_does_not_record_pins_or_reorder_recent(
    cx: &mut TestAppContext,
) {
    init_test_workspace(cx);
    let temp = tempfile::tempdir().expect("create temporary workspace directory");
    let home = temp.path().join("home");
    let pinned_a = temp.path().join("pinned-a");
    let pinned_b = temp.path().join("pinned-b");
    let prior_recent = temp.path().join("prior-recent");
    for dir in [&home, &pinned_a, &pinned_b, &prior_recent] {
        std::fs::create_dir_all(dir).expect("create test directory");
    }
    std::fs::write(pinned_a.join("alpha.txt"), b"a").expect("write pinned-a file");
    std::fs::write(pinned_b.join("beta.txt"), b"b").expect("write pinned-b file");

    let places = Places::for_tests(
        vec![
            Location::from(&pinned_a),
            Location::from(&pinned_b),
            Location::Trash,
        ],
        vec![Location::from(&prior_recent), Location::from(&pinned_a)],
    );

    let (window, browser) = open_test_browser_at(cx, home.clone(), home.clone(), Ok(places));
    wait_for_tabs_loaded(cx, window, &browser);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let view = browser.read(cx);
        assert_eq!(view.tabs.items.len(), 4);
        assert!(view.tabs.items[0].pinned);
        assert!(view.tabs.items[1].pinned);
        assert!(view.tabs.items[2].pinned);
        assert!(!view.tabs.items[3].pinned);
        assert_eq!(view.tabs.active_id, view.tabs.items[3].id);
        assert_eq!(view.current_path().as_deref(), Some(home.as_path()));
        // Restoring pinned tabs must neither append unvisited pins nor re-promote
        // previously visited pins ahead of newer recent entries; unpinned `start`
        // is still recorded.
        assert_eq!(
            view.locations.places.recent,
            vec![
                Location::from(&home),
                Location::from(&prior_recent),
                Location::from(&pinned_a),
            ]
        );
    })
    .expect("verify initial recent list with unpinned start tab");

    // Background file-operation refreshes and manual Refresh must not record
    // unvisited pinned tabs either.
    std::fs::write(pinned_b.join("created.txt"), b"new").expect("write new file in pinned-b");
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.refresh_after_file_operation(&[pinned_b.join("created.txt")], cx);
            browser.dispatch(Command::Refresh, window, cx);
        });
    })
    .expect("trigger background and active refreshes");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.tabs.items.iter().all(|slot| !slot.is_loading())
            && view.tabs.items[1]
                .table_rows
                .iter()
                .any(|entry| entry.display_name == "created.txt")
    });

    cx.update_window(window.into(), |_, _, cx| {
        assert_eq!(
            browser.read(cx).locations.places.recent,
            vec![
                Location::from(&home),
                Location::from(&prior_recent),
                Location::from(&pinned_a),
            ]
        );
    })
    .expect("verify recent list unchanged after background and manual refreshes");

    close_test_browser(cx, window).await;
}

#[gpui_kit::test]
async fn opening_window_when_start_matches_pinned_tab_activates_it_without_recording_recent(
    cx: &mut TestAppContext,
) {
    init_test_workspace(cx);
    let temp = tempfile::tempdir().expect("create temporary workspace directory");
    let home = temp.path().join("home");
    let pinned_b = temp.path().join("pinned-b");
    std::fs::create_dir_all(&home).expect("create home directory");
    std::fs::create_dir_all(&pinned_b).expect("create second pinned directory");
    std::fs::write(home.join("visible.txt"), b"v").expect("write visible file");
    std::fs::write(home.join(".hidden.txt"), b"h").expect("write hidden file");
    std::fs::write(pinned_b.join("other.txt"), b"o").expect("write other file");

    let places = Places::for_tests(
        vec![Location::from(&home), Location::from(&pinned_b)],
        Vec::new(),
    );

    let (window, browser) = open_test_browser_at(cx, home.clone(), home.clone(), Ok(places));
    wait_for_tabs_loaded(cx, window, &browser);

    cx.update_window(window.into(), |_, _, cx| {
        let view = browser.read(cx);
        assert_eq!(view.tabs.items.len(), 2);
        assert!(view.tabs.items.iter().all(|tab| tab.pinned));
        assert_eq!(view.tabs.active_id, view.tabs.items[0].id);
        assert_eq!(view.current_path().as_deref(), Some(home.as_path()));
        assert!(view.locations.places.recent.is_empty());
        assert!(!view.locations.places.has_pending());
    })
    .expect("verify matching pinned start tab activates without recording recent");

    // Refreshing the active pinned tab (F5 / hidden-files reload) also must not
    // record it into recent or queue a places write.
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.dispatch(Command::Refresh, window, cx);
            browser.settings.preferences.files.show_hidden = true;
            browser.refresh_if_hidden_changed(cx);
        });
    })
    .expect("refresh active pinned tab and reload with hidden files");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.tabs.items.iter().all(|slot| !slot.is_loading())
            && view.active().is_some_and(|slot| {
                slot.table_rows
                    .iter()
                    .any(|entry| entry.display_name == ".hidden.txt")
            })
    });

    cx.update_window(window.into(), |_, window, cx| {
        let view = browser.read(cx);
        assert!(view.locations.places.recent.is_empty());
        assert!(!view.locations.places.has_pending());
        window.render_frame(cx);
    })
    .expect("verify recent still empty after active pinned tab refreshes");

    close_test_browser(cx, window).await;
}

#[gpui_kit::test]
async fn navigating_inside_pinned_tab_and_explicit_opens_record_to_recent(cx: &mut TestAppContext) {
    init_test_workspace(cx);
    let temp = tempfile::tempdir().expect("create temporary workspace directory");
    let home = temp.path().join("home");
    let pinned_a = temp.path().join("pinned-a");
    let pinned_child = pinned_a.join("child");
    let other_dir = temp.path().join("other");
    let new_tab_dir = temp.path().join("new-tab");
    for dir in [&home, &pinned_a, &pinned_child, &other_dir, &new_tab_dir] {
        std::fs::create_dir_all(dir).expect("create test directory");
    }

    let places = Places::for_tests(vec![Location::from(&pinned_a)], Vec::new());

    let (window, browser) = open_test_browser_at(cx, pinned_a.clone(), home, Ok(places));
    wait_for_tabs_loaded(cx, window, &browser);

    cx.update_window(window.into(), |_, _, cx| {
        assert!(browser.read(cx).locations.places.recent.is_empty());
    })
    .expect("verify empty recent after restoring active pinned tab");

    // 1. Navigating into a child folder inside the pinned tab records the child.
    cx.update_window(window.into(), |_, _, cx| {
        browser.update(cx, |browser, cx| {
            browser.navigate(pinned_child.clone(), cx);
        });
    })
    .expect("navigate into child folder of pinned tab");

    wait_for_browser(cx, window, &browser, |view, _| {
        !view.tabs.items.iter().any(|slot| slot.is_loading())
            && view.current_path().as_deref() == Some(pinned_child.as_path())
    });
    cx.update_window(window.into(), |_, _, cx| {
        assert_eq!(
            browser.read(cx).locations.places.recent,
            vec![Location::from(&pinned_child)]
        );
    })
    .expect("verify child folder inside pinned tab recorded in recent");

    // 2. History Back navigation records the destination visited by the user.
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.dispatch(Command::Back, window, cx);
        });
    })
    .expect("navigate back to pinned-a");

    wait_for_browser(cx, window, &browser, |view, _| {
        !view.tabs.items.iter().any(|slot| slot.is_loading())
            && view.current_path().as_deref() == Some(pinned_a.as_path())
    });
    cx.update_window(window.into(), |_, _, cx| {
        assert_eq!(
            browser.read(cx).locations.places.recent,
            vec![Location::from(&pinned_a), Location::from(&pinned_child)]
        );
    })
    .expect("verify Back navigation recorded in recent");

    // 3. Explicit sidebar/location navigation records the visited folder.
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.show_location(Location::from(&other_dir), window, cx);
        });
    })
    .expect("navigate explicitly to other_dir");

    wait_for_browser(cx, window, &browser, |view, _| {
        !view.tabs.items.iter().any(|slot| slot.is_loading())
            && view.current_path().as_deref() == Some(other_dir.as_path())
    });
    cx.update_window(window.into(), |_, _, cx| {
        assert_eq!(
            browser.read(cx).locations.places.recent,
            vec![
                Location::from(&other_dir),
                Location::from(&pinned_a),
                Location::from(&pinned_child),
            ]
        );
    })
    .expect("verify explicit show_location recorded in recent");

    // 4. Opening a folder in a new tab via UI records the opened folder, and
    // switching tabs leaves recent untouched.
    let first_tab_id = cx
        .update_window(window.into(), |_, window, cx| {
            let first_id = browser.read(cx).tabs.items[0].id;
            browser.update(cx, |browser, cx| {
                browser.open_location_tab(Location::from(&new_tab_dir), false, Some(window), cx);
            });
            first_id
        })
        .expect("open new_tab_dir in new unpinned tab");

    wait_for_browser(cx, window, &browser, |view, _| {
        !view.tabs.items.iter().any(|slot| slot.is_loading())
            && view.current_path().as_deref() == Some(new_tab_dir.as_path())
    });
    cx.update_window(window.into(), |_, window, cx| {
        assert_eq!(
            browser.read(cx).locations.places.recent,
            vec![
                Location::from(&new_tab_dir),
                Location::from(&other_dir),
                Location::from(&pinned_a),
                Location::from(&pinned_child),
            ]
        );
        browser.update(cx, |browser, cx| {
            browser.switch_tab(first_tab_id, window, cx);
        });
        assert_eq!(browser.read(cx).locations.places.recent.len(), 4);
    })
    .expect("verify open_location_tab recorded and switch_tab did not duplicate");

    close_test_browser(cx, window).await;
}

#[gpui_kit::test]
async fn live_enrichment_and_selection_cow_preserve_pointers_respect_budget_and_settle_type_sort(
    cx: &mut TestAppContext,
) {
    let temp = init_test_workspace(cx);
    let home = temp.path().join("home");
    let plain_dir = temp.path().join("plain");
    std::fs::create_dir_all(&home).expect("create home directory");
    std::fs::create_dir_all(&plain_dir).expect("create plain directory");

    // 100 directories: viewport covers the top rows, while tail rows 85..100 sit off-screen.
    for index in 0..100 {
        let dir = home.join(format!("folder-{index:03}"));
        std::fs::create_dir_all(&dir).expect("create test folder");
    }
    std::fs::write(home.join("folder-095/child-a.txt"), b"a").expect("write tail child 1");
    std::fs::write(home.join("folder-095/child-b.txt"), b"b").expect("write tail child 2");
    std::fs::write(home.join("folder-099/child-a.txt"), b"a").expect("write tail child 3");

    for index in 0..12 {
        std::fs::write(plain_dir.join(format!("doc-{index:02}.txt")), b"plain text").unwrap();
    }

    // Configure off-screen probe_budget = 0 before opening the window so only viewport items probe initially.
    cx.update(|cx| {
        cx.gds_mut()
            .preferences()
            .edit(|prefs| prefs.files.probe_budget = 0)
            .expect("set probe budget");
    });

    let (window, browser) = open_test_browser(
        cx,
        home.clone(),
        Ok(crate::entities::location::places::Places::default()),
    );
    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            !slot.is_loading()
                && slot.table_rows.len() == 100
                && slot.local_enrich().is_some_and(|job| !job.is_in_flight())
        })
    });

    // Verify Repr::Single and Repr::Set CoW across live rendered frames, then capture snapshot pointers.
    let (snapshot_ptr, entries_ptr) = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            browser.update(cx, |browser, cx| {
                browser.window.focus.focus(window, cx);
                browser.select(0, Modifiers::default(), cx);
            });
            window.render_frame(cx);
            let single_ptr = browser
                .read(cx)
                .active()
                .unwrap()
                .navigation
                .view()
                .selected
                .as_slice()
                .as_ptr();

            browser.update(cx, |browser, cx| {
                browser.select(1, Modifiers::default(), cx);
            });
            window.render_frame(cx);
            assert_eq!(
                browser
                    .read(cx)
                    .active()
                    .unwrap()
                    .navigation
                    .view()
                    .selected
                    .as_slice()
                    .as_ptr(),
                single_ptr,
                "single-selection click after rendered frame must reuse Repr::Single allocation in place"
            );

            // Build a 4-item Set selection, render a frame (populating FileTable + DragSnapshot), then Ctrl-deselect 1 item.
            browser.update(cx, |browser, cx| {
                for row in [0, 2, 3] {
                    browser.select(
                        row,
                        Modifiers {
                            control: true,
                            ..Default::default()
                        },
                        cx,
                    );
                }
            });
            window.render_frame(cx);
            let set_ptr = browser
                .read(cx)
                .active()
                .unwrap()
                .navigation
                .view()
                .selected
                .as_slice()
                .as_ptr();

            browser.update(cx, |browser, cx| {
                browser.select(
                    2,
                    Modifiers {
                        control: true,
                        ..Default::default()
                    },
                    cx,
                );
            });
            window.render_frame(cx);
            assert_eq!(
                browser
                    .read(cx)
                    .active()
                    .unwrap()
                    .navigation
                    .view()
                    .selected
                    .as_slice()
                    .as_ptr(),
                set_ptr,
                "Ctrl+Click toggle after rendered frame must mutate Selection::Set in place without cloning"
            );

            let view = browser.read(cx);
            let slot = view.active().expect("active tab");
            let child_need = directory::EnrichmentNeed {
                children: true,
                ..Default::default()
            };
            // With probe_budget = 0, off-screen tail entry 95 has not been probed yet.
            assert!(slot.table_rows[95].needs_enrichment(child_need));
            // Small selections do not materialize a 2N SelectionBacking.
            assert!(slot.row_backing.is_none());

            let dir_arc = slot.navigation.directory_arc().expect("directory snapshot");
            let snapshot_ptr = Arc::as_ptr(&dir_arc);
            let entries_ptr = dir_arc.entries.as_ptr();
            drop(dir_arc);

            browser.update(cx, |browser, cx| {
                let tail = browser.active().expect("active tab").table_rows[85..100].to_vec();
                browser.request_viewport_enrichment(&tail, cx);
            });
            (snapshot_ptr, entries_ptr)
        })
        .expect("verify live Selection CoW, capture pointers, and request tail enrichment");

    let child_need = directory::EnrichmentNeed {
        children: true,
        ..Default::default()
    };
    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            slot.local_enrich().is_some_and(|job| !job.is_in_flight())
                && !slot.table_rows[95].needs_enrichment(child_need)
        })
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let view = browser.read(cx);
        let slot = view.active().expect("active tab");
        let dir_arc = slot.navigation.directory_arc().expect("directory snapshot");
        assert_eq!(
            Arc::as_ptr(&dir_arc),
            snapshot_ptr,
            "live enrichment must mutate DirectorySnapshot in place without cloning"
        );
        assert_eq!(
            dir_arc.entries.as_ptr(),
            entries_ptr,
            "live enrichment must mutate snapshot.entries slice in place without O(N) cloning"
        );
        assert_eq!(
            slot.table_rows[95].details.children,
            Some(directory::ChildCount {
                count: 2,
                truncated: false,
            })
        );

        // Small Shift range (< 64) skips SelectionBacking; large Shift range (>= 64) materializes it.
        browser.update(cx, |browser, cx| {
            browser.select(0, Modifiers::default(), cx);
            browser.select(
                10,
                Modifiers {
                    shift: true,
                    ..Default::default()
                },
                cx,
            );
        });
        assert!(browser.read(cx).active().unwrap().row_backing.is_none());

        browser.update(cx, |browser, cx| {
            browser.select(
                70,
                Modifiers {
                    shift: true,
                    ..Default::default()
                },
                cx,
            );
        });
        assert!(browser.read(cx).active().unwrap().row_backing.is_some());

        // Navigate to `plain_dir` and switch to Sort::Type where probing `.txt` files yields `!changed_visible`.
        browser.update(cx, |browser, cx| {
            browser.navigate(plain_dir.clone(), cx);
        });
    })
    .expect("verify in-place pointers, lazy row_backing threshold, and navigate to plain_dir");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            !slot.is_loading()
                && slot.navigation.path() == Some(plain_dir.as_path())
                && slot.table_rows.len() == 12
        })
    });

    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            browser.set_sort(
                sorting::Sort {
                    key: sorting::Key::Type,
                    direction: sorting::Direction::Ascending,
                },
                cx,
            );
        });
        window.render_frame(cx);
    })
    .expect("switch plain_dir to Type sort");

    let content_need = directory::EnrichmentNeed {
        content: true,
        ..Default::default()
    };
    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            slot.local_enrich().is_some_and(|job| !job.is_in_flight())
                && slot
                    .table_rows
                    .iter()
                    .all(|entry| !entry.needs_enrichment(content_need))
        })
    });

    // Render multiple frames and ensure table rows stay probed so no infinite reschedule occurs.
    for _ in 0..4 {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
        })
        .expect("render settled Type-sorted frame");
        cx.run_until_parked();
    }

    cx.update_window(window.into(), |_, _, cx| {
        let view = browser.read(cx);
        let slot = view.active().expect("active tab");
        assert!(
            slot.table_rows
                .iter()
                .all(|entry| !entry.needs_enrichment(content_need))
        );
        assert!(slot.local_enrich().is_some_and(|job| !job.is_in_flight()));
    })
    .expect("verify Type sort enrichment stays settled without render reschedule loop");

    close_test_browser(cx, window).await;
}

#[gpui_kit::test]
async fn scroll_reentry_caps_offscreen_budget_and_sorted_precheck_skips_redundant_resorts(
    cx: &mut TestAppContext,
) {
    let temp = init_test_workspace(cx);
    let big_dir = temp.path().join("big");
    let size_dir = temp.path().join("size_sorted");
    let type_resort_dir = temp.path().join("type_resort");
    let grid_dir = temp.path().join("grid_folder");
    std::fs::create_dir_all(&big_dir).unwrap();
    std::fs::create_dir_all(&size_dir).unwrap();
    std::fs::create_dir_all(&type_resort_dir).unwrap();
    std::fs::create_dir_all(&grid_dir).unwrap();

    for index in 0..120 {
        std::fs::create_dir_all(big_dir.join(format!("item-{index:03}"))).unwrap();
    }

    // Extensionless regular files: content probing visibly updates `details.content` (`None` -> `text/plain`),
    // while `Size` ordering is already fixed by `lstat` byte lengths (1..=80 bytes).
    for index in 0..80 {
        let payload = "x".repeat(index + 1);
        std::fs::write(
            size_dir.join(format!("blob-{index:03}")),
            payload.as_bytes(),
        )
        .unwrap();
    }

    // `alpha.png` (Image category) and extensionless `zeta` containing UTF-8 text (Text category):
    // before probing, `zeta` has `None` content (sorts after `Some(Image)`); after probing, `Text` sorts before `Image`.
    std::fs::write(
        type_resort_dir.join("alpha.png"),
        b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde",
    )
    .unwrap();
    std::fs::write(type_resort_dir.join("zeta"), b"hello plain text").unwrap();

    std::fs::write(grid_dir.join("pic.txt"), b"hi").unwrap();

    cx.update(|cx| {
        cx.gds_mut()
            .preferences()
            .edit(|prefs| prefs.files.probe_budget = 8)
            .expect("set probe_budget = 8");
    });

    let mut places = crate::entities::location::places::Places::default();
    places.set_sort(
        Location::from(type_resort_dir.as_path()),
        sorting::Sort {
            key: sorting::Key::Type,
            direction: sorting::Direction::Ascending,
        },
    );
    places.set_view_mode(Location::from(grid_dir.as_path()), ViewMode::Grid);

    let (window, browser) = open_test_browser(cx, big_dir.clone(), Ok(places));
    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            !slot.is_loading()
                && slot.table_rows.len() == 120
                && slot.local_enrich().is_some_and(|job| !job.is_in_flight())
        })
    });

    let child_need = directory::EnrichmentNeed {
        children: true,
        ..Default::default()
    };

    // Initial pass probed default viewport (0..64) plus `probe_budget = 8` off-screen entries (64..72).
    cx.update_window(window.into(), |_, _, cx| {
        let slot = browser.read(cx).active().expect("active tab");
        assert!(!slot.table_rows[71].needs_enrichment(child_need));
        assert!(slot.table_rows[72].needs_enrichment(child_need));
        assert!(slot.table_rows[110].needs_enrichment(child_need));

        browser.update(cx, |browser, cx| {
            let scrolled_slice = browser.active().unwrap().table_rows[110..115].to_vec();
            browser.request_viewport_enrichment(&scrolled_slice, cx);
        });
    })
    .expect("trigger scroll re-entry enrichment on tail slice");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            slot.local_enrich().is_some_and(|job| !job.is_in_flight())
                && !slot.table_rows[114].needs_enrichment(child_need)
        })
    });

    cx.update_window(window.into(), |_, _, cx| {
        let slot = browser.read(cx).active().expect("active tab");
        assert!(
            slot.table_rows[72].needs_enrichment(child_need),
            "scroll re-entry must not spend an extra off-screen probe_budget pass on rows 72..80"
        );

        browser.update(cx, |browser, cx| {
            browser.navigate(size_dir.clone(), cx);
        });
    })
    .expect("verify scroll re-entry budget cap and navigate to size_dir");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            !slot.is_loading()
                && slot.navigation.path() == Some(size_dir.as_path())
                && slot.table_rows.len() == 80
                && slot.local_enrich().is_some_and(|job| !job.is_in_flight())
        })
    });

    let content_need = directory::EnrichmentNeed {
        content: true,
        ..Default::default()
    };

    // Tail entries 72..80 are still unprobed (`probe_budget = 8` after 0..64 viewport).
    // Switch synchronously to `Sort::Size` and materialize `row_backing` before async probing flushes.
    let size_rows_ptr = cx
        .update_window(window.into(), |_, _, cx| {
            assert!(
                browser.read(cx).active().unwrap().table_rows[75].needs_enrichment(content_need)
            );
            browser.update(cx, |browser, cx| {
                browser.set_sort(
                    sorting::Sort {
                        key: sorting::Key::Size,
                        direction: sorting::Direction::Ascending,
                    },
                    cx,
                );
                browser.select(0, Modifiers::default(), cx);
                browser.select(
                    70,
                    Modifiers {
                        shift: true,
                        ..Default::default()
                    },
                    cx,
                );
            });
            let slot = browser.read(cx).active().unwrap();
            assert!(slot.row_backing.is_some());
            slot.table_rows.as_ptr()
        })
        .expect("switch to Size sort and materialize row_backing before tail probes complete");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            slot.local_enrich().is_some_and(|job| !job.is_in_flight())
                && slot
                    .table_rows
                    .iter()
                    .all(|entry| !entry.needs_enrichment(content_need))
        })
    });

    cx.update_window(window.into(), |_, _, cx| {
        let slot = browser.read(cx).active().expect("active tab");
        assert!(
            slot.row_backing.is_some(),
            "order pre-check must preserve row_backing when Size sort order is unbroken"
        );
        assert_eq!(
            slot.table_rows.as_ptr(),
            size_rows_ptr,
            "order pre-check must mutate Size-sorted table_rows in place without O(N log N) reallocation"
        );

        browser.update(cx, |browser, cx| {
            browser.navigate(type_resort_dir.clone(), cx);
        });
    })
    .expect("verify Size-sorted in-place update and navigate to type_resort_dir");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            !slot.is_loading()
                && slot.navigation.path() == Some(type_resort_dir.as_path())
                && slot.local_enrich().is_some_and(|job| !job.is_in_flight())
                && slot
                    .table_rows
                    .iter()
                    .all(|entry| !entry.needs_enrichment(content_need))
        })
    });

    cx.update_window(window.into(), |_, _, cx| {
        let slot = browser.read(cx).active().expect("active tab");
        assert_eq!(slot.table_rows[0].display_name, "zeta");
        assert_eq!(slot.table_rows[1].display_name, "alpha.png");
        assert!(slot.sort.is_sorted(&slot.table_rows));

        browser.update(cx, |browser, cx| {
            browser.navigate(grid_dir.clone(), cx);
        });
    })
    .expect("verify Type resort when probed MIME crosses boundary and navigate to grid_dir");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            !slot.is_loading() && slot.navigation.path() == Some(grid_dir.as_path())
        })
    });

    // Without rendering a frame, `finish_request` has already synchronized `slot.view_mode`.
    cx.update_window(window.into(), |_, _, cx| {
        let slot = browser.read(cx).active().expect("active tab");
        assert_eq!(slot.view_mode, ViewMode::Grid);
    })
    .expect("verify finish_request syncs Grid view_mode immediately");

    close_test_browser(cx, window).await;
}

#[gpui_kit::test]
async fn progressive_streaming_loads_preserve_selection_and_respect_preferences(
    cx: &mut TestAppContext,
) {
    let temp = init_test_workspace(cx);
    let home = temp.path().join("home");
    let stream_dir = home.join("usb_slow_dir");
    std::fs::create_dir_all(&stream_dir).expect("create stream fixture directory");
    for index in 0..40 {
        std::fs::write(stream_dir.join(format!("item-{index:03}.txt")), "payload")
            .expect("write stream fixture file");
    }

    let (window, browser) =
        open_test_browser_at(cx, home.clone(), home.clone(), Ok(Places::default()));
    wait_for_tabs_loaded(cx, window, &browser);

    // Configure streaming and enrichment parameters via Preferences rather than magic constants.
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |browser, cx| {
            let mut prefs = browser.settings.preferences.clone();
            prefs.files.streaming.initial_delay_ms = 0;
            prefs.files.streaming.interval_ms = 0;
            prefs.files.streaming.min_entries = 10;
            prefs.files.enrichment.priority_limit = 64;
            prefs.files.enrichment.coalesce_ms = 0;
            prefs.files.enrichment.reorder_coalesce_ms = 0;
            prefs.files.enrichment.coalesce_max_updates = 64;
            prefs.files.enrichment.max_concurrent_batches = 2;
            browser.apply_preferences(prefs, window, cx);
            browser.navigate(stream_dir.clone(), cx);
        });
    })
    .expect("configure streaming/enrichment preferences and navigate to stream_dir");

    wait_for_browser(cx, window, &browser, |view, _| {
        view.active().is_some_and(|slot| {
            !slot.is_loading()
                && slot.navigation.path() == Some(stream_dir.as_path())
                && slot.table_rows.len() == 40
        })
    });

    // Deterministically verify intermediate partial snapshots during refresh:
    // a selected tail item (`item-035.txt`) and `pending_selection` (`item-038.txt`) that have not
    // yet appeared in early partial batches must survive until `finish_request` completes.
    let selected_tail = stream_dir.join("item-035.txt");
    let pending_tail = stream_dir.join("item-038.txt");
    cx.update_window(window.into(), |_, _, cx| {
        browser.update(cx, |browser, cx| {
            let id = browser.tabs.active_id;
            let options = browser.read_options(id);
            let cancel = tokio_util::sync::CancellationToken::new();
            let full_snapshot = Arc::new(
                directory::read_directory_base(&stream_dir, options, &cancel)
                    .expect("read full snapshot"),
            );
            let partial_10 = Arc::new(directory::DirectorySnapshot {
                path: stream_dir.clone(),
                entries: full_snapshot.entries[..10].into(),
                warnings: Vec::new(),
                options,
            });
            let partial_25 = Arc::new(directory::DirectorySnapshot {
                path: stream_dir.clone(),
                entries: full_snapshot.entries[..25].into(),
                warnings: Vec::new(),
                options,
            });

            let slot = browser.tab_slot_mut(id).expect("active tab");
            slot.navigation
                .view_mut()
                .selected
                .set_single(selected_tail.clone());
            slot.cursor = Some(selected_tail.clone());
            slot.pending_selection = Some(pending_tail.clone());
            let refresh_req = slot
                .navigation
                .prepare_refresh(Some(stream_dir.clone()), options);

            assert!(browser.apply_partial_request(id, &refresh_req, partial_10, None, cx));
            let slot = browser
                .tab_slot(id)
                .expect("active tab after first partial");
            assert!(slot.is_loading());
            assert_eq!(slot.table_rows.len(), 10);
            assert!(slot.navigation.view().selected.contains(&selected_tail));
            assert_eq!(slot.cursor.as_ref(), Some(&selected_tail));
            assert_eq!(slot.pending_selection.as_ref(), Some(&pending_tail));

            assert!(browser.apply_partial_request(id, &refresh_req, partial_25, None, cx));
            let slot = browser
                .tab_slot(id)
                .expect("active tab after second partial");
            assert!(slot.is_loading());
            assert_eq!(slot.table_rows.len(), 25);
            assert!(slot.navigation.view().selected.contains(&selected_tail));
            assert_eq!(slot.pending_selection.as_ref(), Some(&pending_tail));

            browser.finish_request(id, &refresh_req, Ok(Some(full_snapshot)), None, cx);
            let slot = browser
                .tab_slot(id)
                .expect("active tab after final snapshot");
            assert!(!slot.is_loading());
            assert_eq!(slot.table_rows.len(), 40);
            // Once loading completes, `pending_selection` is applied cleanly.
            assert!(slot.pending_selection.is_none());
            assert!(slot.navigation.view().selected.contains(&pending_tail));
            assert_eq!(slot.cursor.as_ref(), Some(&pending_tail));
        });
    })
    .expect("verify selection and pending_selection survive progressive partial snapshots");

    close_test_browser(cx, window).await;
}
