use gpui_kit::{TestAppContext, component::Root, test::TestWindowExt};

#[gpui_kit::test]
fn resizing_preserves_preferences_and_keeps_compact_actions_usable(cx: &mut TestAppContext) {
    use super::*;
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("workspace fixture");
    std::fs::write(directory.path().join("example.txt"), "example").expect("fixture file");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("unpolled runtime");
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("translations");
        crate::features::file_operations::commands::register(cx);
    });
    let mut browser = None;
    let window = cx.open_window(size(px(1400.), px(740.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut places = Places::default();
            places.set_details(Location::from(directory.path()), true);
            Browser::new(
                LaunchOptions {
                    runtime: runtime.handle().clone(),
                    places: Ok(places),
                    start: directory.path().to_owned(),
                    home: directory.path().to_owned(),
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
    let browser = browser.expect("workspace");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.activate_window();
        assert!(browser.read(cx).details_visible());
        assert_eq!(window.find("page-title").label(), Some("Home"));
        assert!(!browser.read(cx).sidebar_collapsed());
        assert!(window.try_find("back").is_some());
    })
    .expect("wide layout");
    // Exercise the entire expanded range, including the former title-hiding interval.
    for width in [1280., 1180., 1100.] {
        cx.simulate_window_resize(window.into(), size(px(width), px(740.)));
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(!browser.read(cx).window.layout.compact);
            assert!(browser.read(cx).details_visible());
            let title = window.find("page-title");
            let filter = window.find("directory-filter").bounds();
            assert_eq!(title.label(), Some("Home"));
            assert!(title.visible());
            assert!(title.bounds().size.width > px(40.));
            assert!(title.bounds().right() <= filter.left());
            assert!(window.find("refresh").visible());
            assert!(window.try_find("page-tools").is_none());
        })
        .expect("expanded header always retains the directory title");
    }
    cx.simulate_window_resize(window.into(), gds::ui::window::minimum_size());
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(browser.read(cx).sidebar_collapsed());
        assert!(!browser.read(cx).sidebar.collapsed);
        assert!(!browser.read(cx).details_visible());
        for id in [
            "back", "forward", "parent", "sidebar", "details", "sort", "refresh",
        ] {
            assert!(
                window.try_find(id).is_none(),
                "{id} must move out of compact layout"
            );
        }
        let filter = window.find("directory-filter").bounds();
        assert!(filter.size.width >= px(96.));
        assert_eq!(window.find("page-title").label(), Some("Home"));
        let title = window.find("page-title").bounds();
        let tools = window.find("page-tools").bounds();
        assert!(title.size.width > px(40.));
        assert!(title.right() <= filter.left());
        assert!(filter.right() <= tools.left());
        assert!(tools.right() <= window.bounds().size.width);
        assert_eq!(tools.center().y, filter.center().y);
        assert!(window.find("application-menu").visible());
        browser.update(cx, |browser, cx| {
            browser.dispatch(Command::Details, window, cx);
            browser.dispatch(Command::Sidebar, window, cx);
        });
        assert!(!browser.read(cx).details_visible());
        assert!(!browser.read(cx).sidebar.collapsed);
        window.click("application-menu", cx);
    })
    .expect("compact layout");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        assert_eq!(browser.read(cx).menus.open, Some(menu::Kind::Application));
        assert!(
            window
                .try_find(i18n::text(Command::Hidden.label_id(), cx))
                .is_none()
        );
        window.click("page-tools", cx);
    })
    .expect("the fixed main menu remains distinct from page tools");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        assert_eq!(browser.read(cx).menus.open, Some(menu::Kind::PageTools));
        window.press("alt-m", cx);
    })
    .expect("main-menu shortcut switches away from page tools");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        assert_eq!(browser.read(cx).menus.open, Some(menu::Kind::Application));
        window.press("escape", cx);
    })
    .expect("dismiss the main menu");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        assert_eq!(browser.read(cx).menus.open, None);
        window.click("page-tools", cx);
    })
    .expect("open compact page actions");
    cx.run_until_parked();
    // Run the actual compact-menu action, including dismissal and focus restoration.
    let hidden = cx.update(|cx| browser.read(cx).settings.preferences.files.show_hidden);
    cx.update_window(window.into(), |_, window, cx| {
        let label = i18n::text(Command::Hidden.label_id(), cx);
        window.click(label, cx);
    })
    .expect("toggle hidden from compact menu");
    cx.run_until_parked();
    cx.update(|cx| {
        assert_ne!(
            browser.read(cx).settings.preferences.files.show_hidden,
            hidden
        )
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.click("page-tools", cx);
        window.within("popup-menu").click(6usize, cx);
    })
    .expect("open nested sorting menu");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.press("down", cx);
        window.press("enter", cx);
    })
    .expect("choose type sorting");
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            browser.read(cx).active().expect("folder").sort.key,
            sorting::Key::Type
        )
    });
    cx.simulate_window_resize(window.into(), size(px(1400.), px(740.)));
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(browser.read(cx).details_visible());
        assert!(!browser.read(cx).sidebar_collapsed());
        assert!(window.try_find("back").is_some());
        window.click("sidebar", cx);
    })
    .expect("restore expanded layout");
    cx.simulate_window_resize(window.into(), size(px(700.), px(420.)));
    cx.run_until_parked();
    cx.simulate_window_resize(window.into(), size(px(1400.), px(740.)));
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(
            browser.read(cx).sidebar_collapsed(),
            "keep manual collapse preference"
        )
    });
    cx.simulate_window_resize(window.into(), size(px(700.), px(420.)));
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.click("page-tools", cx);
        window.within("popup-menu").click(8usize, cx);
    })
    .expect("open nested application menu");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.click(i18n::text(Command::Settings.label_id(), cx), cx);
    })
    .expect("open settings from submenu");
    cx.run_until_parked();
    cx.update(|cx| assert!(browser.read(cx).pages.current() == Some(&pages::Page::Settings)));
}
