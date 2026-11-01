use gpui_kit::{component::Root, test::TestWindowExt};

#[gpui_kit::test]
fn mixed_favorites_share_selection_copy_open_and_remove(cx: &mut gpui_kit::TestAppContext) {
    use super::*;
    cx.executor().allow_parking();
    let root = tempfile::tempdir().expect("places fixture");
    let local = Location::from(root.path());
    let remote = Location::from_uri("sftp://host/team").expect("remote URI");
    // Keep connection/discovery tasks unpolled; opening a remote row only requests navigation.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("translations");
        crate::features::file_operations::commands::register(cx);
    });
    let mut browser = None;
    let window = cx.open_window(size(px(1100.), px(740.)), |window, cx| {
        let mut places = Places::default();
        for location in [&local, &remote, &Location::Volumes] {
            places.set_favorite(location.clone(), true);
        }
        let view = cx.new(|cx| {
            Browser::new(
                LaunchOptions {
                    runtime: runtime.handle().clone(),
                    places: Ok(places),
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
    let browser = browser.expect("browser");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |view, cx| {
            view.show_location(Location::Favorites, window, cx)
        });
        window.activate_window();
        window.render_frame(cx);
        for (index, location) in [&local, &remote, &Location::Volumes]
            .into_iter()
            .enumerate()
        {
            window.click(("location-row", index), cx);
            assert_eq!(browser.read(cx).selected_place().as_ref(), Some(location));
            window.press("ctrl-shift-c", cx);
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some(location.uri().expect("URI"))
            );
        }
        window.press("enter", cx);
        assert_eq!(browser.read(cx).current_location(), Location::Volumes);
        window.press("alt-left", cx);
        assert_eq!(browser.read(cx).selected_place(), Some(Location::Volumes));
        window.press("up", cx);
        assert_eq!(browser.read(cx).selected_place(), Some(remote.clone()));
        window.press("ctrl-enter", cx);
        assert_eq!(browser.read(cx).tabs.len(), 2);
        assert_eq!(
            browser
                .read(cx)
                .current_tab()
                .expect("remote tab")
                .navigation
                .pending_location(),
            Some(&remote)
        );
        browser.update(cx, |view, cx| view.cancel_location_load(cx));
    })
    .expect("mixed location interaction");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |view, cx| {
            view.show_location(Location::Favorites, window, cx)
        });
        window.render_frame(cx);
        window.click(("location-row", 1usize), cx);
        window.press("delete", cx);
        assert!(!browser.read(cx).places.is_favorite(&remote));
        assert!(browser.read(cx).places.is_favorite(&local));
        assert!(root.path().exists());
        browser.update(cx, |view, cx| {
            view.show_location(Location::Network, window, cx)
        });
        window.render_frame(cx);
        window.press("ctrl-d", cx);
        assert!(browser.read(cx).places.is_favorite(Location::Network));
        assert!(browser.read(cx).places.recent.contains(&Location::Network));
    })
    .expect("remove bookmark and bookmark a virtual page");
}
