use crate::pages::workspace::ui::{Browser, LaunchOptions, Location, Places, configuration};
use gpui_kit::{
    AppContext, MouseButton, TestAppContext,
    component::Root,
    px, size,
    test::{TestEventExt, TestWindowExt},
};

#[gpui_kit::test]
fn right_click_navigation_lists_visits_without_navigating(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("history directory");
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
            Browser::new(
                LaunchOptions {
                    runtime: runtime.handle().clone(),
                    places: Ok(Places::default()),
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
    let browser = browser.expect("browser");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |view, cx| {
            view.show_location(Location::Recent, window, cx);
            view.show_location(Location::Favorites, window, cx);
            view.show_location(Location::Network, window, cx);
        });
        window.activate_window();
    })
    .expect("visit history");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| window.click("back", cx))
        .expect("left click");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        let view = browser.read(cx);
        assert_eq!(view.current_location(), Location::Favorites);
        assert!(window.try_find("popup-menu").is_none());
        window.press("alt-right", cx);
        assert_eq!(browser.read(cx).current_location(), Location::Network);
    })
    .expect("forward shortcut");
    cx.run_until_parked();

    for (button, destination, index) in [
        ("back", Some("Recent"), 1),
        ("forward", Some("Network"), 3),
        ("back", None, 3),
    ] {
        let original = cx
            .update_window(window.into(), |_, window, cx| {
                let original = browser.read(cx).current_location();
                window.right_click(button, cx);
                original
            })
            .expect("right-click navigation button");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            assert!(window.find("popup-menu").visible());
            assert_eq!(
                browser.read(cx).current_location(),
                original,
                "opening the list must not navigate"
            );
            if let Some(destination) = destination {
                let position = window
                    .within("popup-menu")
                    .find_by_label(destination)
                    .bounds()
                    .center();
                window.pointer_move(position, None, cx);
                window.pointer_down(position, MouseButton::Left, cx);
                window.pointer_up(position, MouseButton::Left, cx);
            } else {
                window.press("escape", cx);
            }
        })
        .expect("choose a history visit");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            let view = browser.read(cx);
            assert!(window.try_find("popup-menu").is_none());
            let navigation = &view.current_tab().expect("tab").navigation;
            assert_eq!(navigation.history_index(), index);
            assert_eq!(
                navigation.history().len(),
                4,
                "a jump does not append a visit"
            );
        })
        .expect("history menu dismissed");
    }

    cx.update_window(window.into(), |_, window, cx| {
        let position = window.find("forward").bounds().center();
        window.pointer_move(position, None, cx);
        window.pointer_down(position, MouseButton::Right, cx);
        window.pointer_up(position, MouseButton::Right, cx);
        assert!(
            window.try_find("popup-menu").is_none(),
            "disabled directions have no menu"
        );
        assert_eq!(browser.read(cx).current_location(), Location::Network);
        window.press("alt-left", cx);
        assert_eq!(browser.read(cx).current_location(), Location::Favorites);
    })
    .expect("disabled history and focus restoration");
}
