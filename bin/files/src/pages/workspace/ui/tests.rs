use super::cycle_neighbour;

#[gpui_kit::test]
fn tab_reaches_collapsed_locations_and_file_navigation(cx: &mut gpui_kit::TestAppContext) {
    use super::*;
    use gpui_kit::{component::Root, test::TestWindowExt};

    cx.executor().allow_parking();
    let root = tempfile::tempdir().expect("workspace fixture");
    for name in ["a", "b"] {
        std::fs::create_dir(root.path().join(name)).expect("child directory");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("unpolled device runtime");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("translations");
        crate::features::file_operations::commands::register(cx);
    });
    let mut browser = None;
    let window = cx.open_window(size(px(1100.), px(740.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut preferences = configuration::Preferences::default();
            preferences.files.sidebar.locations_collapsed = true;
            Browser::new(
                LaunchOptions {
                    runtime: runtime.handle().clone(),
                    places: Ok(Places::default()),
                    start: root.path().to_owned(),
                    home: root.path().to_owned(),
                    preferences,
                    startup_error: None,
                    picker: None,
                },
                window,
                cx,
            )
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.expect("workspace");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.activate_window();
        window.click("back", cx);
        window.click("edit-path", cx);
        assert!(browser.read(cx).toolbar.path.editing);
        window.press("escape", cx);
        window.click("application-menu", cx);
        assert!(browser.read(cx).menus.application_open);
        window.press("escape", cx);
        assert!(!browser.read(cx).menus.application_open);
        window.press("ctrl-l", cx);
        let mut reached_locations = false;
        let mut reached_files = false;
        for _ in 0..64 {
            window.press("tab", cx);
            if browser
                .read(cx)
                .sidebar
                .regions
                .locations
                .is_focused(window)
            {
                reached_locations = true;
                window.press("down", cx);
                assert!(browser.read(cx).sidebar.regions.location == LocationTarget::Recent);
            }
            if browser.read(cx).window.focus.is_focused(window) {
                reached_files = true;
                window.press("down", cx);
                assert_eq!(
                    browser.read(cx).selected_visible_paths(),
                    [root.path().join("a")]
                );
                window.press("down", cx);
                assert_eq!(
                    browser.read(cx).selected_visible_paths(),
                    [root.path().join("b")]
                );
                window.press("up", cx);
                assert_eq!(
                    browser.read(cx).selected_visible_paths(),
                    [root.path().join("a")]
                );
            }
            if reached_locations && reached_files {
                break;
            }
        }
        assert!(
            reached_locations,
            "Tab must reach the collapsed locations group"
        );
        assert!(reached_files, "Tab must enter file selection");
        window.press("alt-l", cx);
        window.press("home", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .expect("keyboard navigation");
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(browser.read(cx).current_location(), Location::Recent);
    });
}

/// The page-cycle keys and the per-tab hints share one target function: the
/// strip order is the cycle order, the ends connect, one tab has no neighbour
/// and two tabs point both directions at the same row.
#[test]
fn a_page_cycle_lands_on_the_neighbour_the_tab_hint_names() {
    // Pinned tabs sort to the front, so the strip order is what cycles.
    let strip = [7, 3, 5, 9];

    assert_eq!(cycle_neighbour(&strip, 7, false), Some(3));
    assert_eq!(cycle_neighbour(&strip, 9, false), Some(7));
    assert_eq!(cycle_neighbour(&strip, 7, true), Some(9));
    assert_eq!(cycle_neighbour(&strip, 5, true), Some(3));

    assert_eq!(cycle_neighbour(&[4], 4, false), None);
    assert_eq!(cycle_neighbour(&[4], 4, true), None);

    assert_eq!(cycle_neighbour(&[1, 2], 1, false), Some(2));
    assert_eq!(cycle_neighbour(&[1, 2], 1, true), Some(2));
    assert_eq!(cycle_neighbour(&[1, 2], 2, false), Some(1));
}

#[test]
fn desktop_reveal_applies_a_group_selection_after_the_directory_loads() {
    use super::*;
    let root = tempfile::tempdir().expect("reveal directory");
    for name in ["one", "two", "other"] {
        std::fs::write(root.path().join(name), []).expect("item");
    }
    let mut slot = OpenTab::new(
        1,
        Location::from(root.path()),
        Some(root.path().to_owned()),
        false,
    );
    slot.pending_reveal = Some(vec![root.path().join("one"), root.path().join("two")]);
    slot.refresh_visible();
    assert!(
        slot.pending_reveal.is_some(),
        "an unfinished load must keep the request"
    );
    let request = slot
        .navigation
        .prepare_refresh(Some(root.path().to_owned()), ReadOptions::default());
    slot.navigation
        .finish_load(&request, request.load())
        .expect("directory load");
    slot.refresh_visible();
    assert_eq!(
        slot.navigation.view().selected,
        [root.path().join("one"), root.path().join("two")]
    );
}

#[gpui_kit::test]
fn sidebar_locations_and_addresses_navigate_one_tab_and_share_back_forward(
    cx: &mut gpui_kit::TestAppContext,
) {
    use super::*;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;
    cx.executor().allow_parking();
    let root = tempfile::tempdir().expect("workspace fixture");
    let mount = root.path().join("volume");
    std::fs::create_dir(&mount).expect("volume root");
    // Keep device discovery unpolled so this test never connects to the system bus.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("I/O runtime");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("translations");
        crate::features::file_operations::commands::register(cx);
    });
    let mut browser = None;
    let window = cx.open_window(size(px(1100.), px(740.)), |window, cx| {
        let view = cx.new(|cx| {
            Browser::new(
                LaunchOptions {
                    runtime: runtime.handle().clone(),
                    places: Ok(Places::default()),
                    start: root.path().to_owned(),
                    home: root.path().to_owned(),
                    preferences: configuration::Preferences::default(),
                    startup_error: None,
                    picker: None,
                },
                window,
                cx,
            )
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.expect("workspace view");
    cx.run_until_parked();
    let id = storage::VolumeId {
        stable: Some("fixture-volume".into()),
        attachment: "fixture-attachment".into(),
    };
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |this, cx| {
            let storage = this.devices.service.clone();
            storage.update(cx, |storage, cx| {
                storage.snapshot = storage::Snapshot {
                    volumes: vec![storage::Volume {
                        id: id.clone(),
                        object: "fixture".into(),
                        drive: None,
                        name: "Volume".into(),
                        size: 0,
                        label: "Volume".into(),
                        filesystem: "test".into(),
                        read_only: false,
                        locked: false,
                        mountable: true,
                        system_mount: false,
                        mount_points: vec![mount.clone()],
                        visible: true,
                    }],
                    ..Default::default()
                };
                cx.notify();
            });
            assert_eq!(this.tabs.items.len(), 1);
            this.show_location(Location::Recent, window, cx);
            this.show_location(Location::Favorites, window, cx);
            this.show_volume(id.clone(), window, cx);
        })
    })
    .expect("sidebar navigation");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, _, cx| {
        browser.update(cx, |this, cx| {
            assert_eq!(this.tabs.items.len(), 1);
            assert!(matches!(this.current_location(), Location::Volume { .. }));
            assert_eq!(this.current_path(), Some(mount.clone()));
            this.navigate_history(Command::Back, cx);
            assert_eq!(this.current_location(), Location::Favorites);
            this.navigate_history(Command::Back, cx);
            assert_eq!(this.current_location(), Location::Recent);
            this.navigate_history(Command::Back, cx);
        })
    })
    .expect("history across virtual pages");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, _, cx| {
        browser.update(cx, |this, cx| {
            assert_eq!(this.current_path(), Some(root.path().to_owned()));
            this.navigate_history(Command::Forward, cx);
            assert_eq!(this.current_location(), Location::Recent);
            this.navigate_history(Command::Forward, cx);
            assert_eq!(this.current_location(), Location::Favorites);
            this.navigate_history(Command::Forward, cx);
        })
    })
    .expect("forward history");
    cx.run_until_parked();
    cx.update(|cx| {
        let this = browser.read(cx);
        assert_eq!(this.tabs.items.len(), 1);
        assert_eq!(this.current_path(), Some(mount));
    });

    for (address, location) in [
        ("location://recent", Location::Recent),
        ("location://volumes", Location::Volumes),
        ("location://network", Location::Network),
        ("location://trash", Location::Trash),
        ("location://favorites", Location::Favorites),
    ] {
        cx.update_window(window.into(), |_, window, cx| {
            window.activate_window();
            window.press("ctrl-l", cx);
            window.input(address, cx);
            window.press("enter", cx);
        })
        .expect("enter virtual address");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            assert_eq!(browser.read(cx).current_location(), location);
            assert!(!browser.read(cx).toolbar.path.editing);
            window.click("edit-path", cx);
            let this = browser.read(cx);
            assert!(this.toolbar.path.editing);
            assert_eq!(this.toolbar.path.input.read(cx).value().as_ref(), address);
            window.press("escape", cx);
            assert!(!browser.read(cx).toolbar.path.editing);
            assert_eq!(browser.read(cx).current_location(), location);
        })
        .expect("reopen and cancel address editing");
        cx.run_until_parked();
    }

    cx.update_window(window.into(), |_, window, cx| {
        window.press("alt-left", cx);
        assert_eq!(browser.read(cx).current_location(), Location::Trash);
        window.press("alt-right", cx);
        assert_eq!(browser.read(cx).current_location(), Location::Favorites);
        let history_len = browser
            .read(cx)
            .current_tab()
            .expect("tab")
            .navigation
            .history()
            .len();
        window.press("ctrl-l", cx);
        window.input("location://missing", cx);
        window.press("enter", cx);
        let this = browser.read(cx);
        assert!(this.toolbar.path.editing);
        assert_eq!(this.current_location(), Location::Favorites);
        assert_eq!(
            this.current_tab().expect("tab").navigation.history().len(),
            history_len
        );
        window.press("ctrl-l", cx);
        window.input(&root.path().display().to_string(), cx);
        window.press("enter", cx);
    })
    .expect("address history and invalid input");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        assert_eq!(
            browser.read(cx).current_path().as_deref(),
            Some(root.path())
        );
        assert_eq!(browser.read(cx).tabs.items.len(), 1);
        window.press("ctrl-l", cx);
        assert_eq!(
            browser
                .read(cx)
                .toolbar
                .path
                .input
                .read(cx)
                .value()
                .as_ref(),
            root.path().to_string_lossy()
        );
    })
    .expect("home keeps its filesystem address");
}
