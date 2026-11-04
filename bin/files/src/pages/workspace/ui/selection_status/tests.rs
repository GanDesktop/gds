use gpui_kit::{TestAppContext, component::Root, test::TestWindowExt};

#[gpui_kit::test]
fn clicking_remove_deselects_only_that_item_and_refreshes_the_open_list(cx: &mut TestAppContext) {
    use super::*;
    cx.executor().allow_parking();
    let root = tempfile::tempdir().expect("workspace fixture");
    for name in ["a", "b"] {
        std::fs::create_dir(root.path().join(name)).expect("child directory");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("translations");
        crate::features::file_operations::commands::register(cx);
    });
    let mut browser = None;
    let window = cx.open_window(size(px(920.), px(600.)), |window, cx| {
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
    let browser = browser.expect("workspace");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.activate_window();
        browser.update(cx, |browser, cx| {
            browser.window.focus.focus(window, cx);
            browser.select_all_files(window, cx);
        });
        assert_eq!(browser.read(cx).selected_visible_paths().len(), 2);
        window.click("selected-files-status", cx);
    })
    .expect("open selected items");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.hover("selected-0", cx);
        window.within("selected-0").click("deselect-entry", cx);
        assert_eq!(
            browser.read(cx).selected_visible_paths(),
            [root.path().join("b")]
        );
    })
    .expect("remove first selection");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        assert!(window.try_find("selected-1").is_none());
        window.hover("selected-0", cx);
        window.within("selected-0").click("deselect-entry", cx);
        assert!(browser.read(cx).selected_visible_paths().is_empty());
    })
    .expect("remove remaining selection without reopening");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("selected-0").is_none());
    })
    .expect("popover displays empty selection");
}
