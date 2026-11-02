use super::super::{Browser, LaunchOptions, Places, configuration};
use gpui_kit::{
    AppContext, TestAppContext,
    component::{Root, WindowExt},
    px, size,
    test::TestWindowExt,
};

#[gpui_kit::test]
fn pinned_tabs_require_unpinning_and_ordinary_tabs_close_directly(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("tab fixture");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).unwrap();
        crate::features::file_operations::commands::register(cx);
    });
    let mut browser = None;
    let handle = cx.open_window(size(px(1100.), px(800.)), |window, cx| {
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
    let browser = browser.unwrap();
    cx.run_until_parked();
    let pinned = cx
        .update_window(handle.into(), |_, window, cx| {
            let pinned = browser.read(cx).tabs.active_id;
            browser.update(cx, |browser, cx| {
                browser.toggle_pin(pinned, cx);
                browser.window.focus.focus(window, cx);
            });
            window.render_frame(cx);
            assert!(window.find(("unpin-tab", pinned)).visible());
            window.hover(("tab", pinned), cx);
            assert!(window.try_find(("close-tab", pinned)).is_none());
            window.press("ctrl-n", cx);
            window.render_frame(cx);
            let ordinary = browser.read(cx).tabs.active_id;
            window.press("ctrl-w", cx);
            assert!(!window.has_active_dialog(cx));
            assert!(browser.read(cx).tab_slot(ordinary).is_none());
            assert_eq!(browser.read(cx).tabs.active_id, pinned);
            window.render_frame(cx);
            window.press("ctrl-w", cx);
            window.render_frame(cx);
            assert!(window.has_active_dialog(cx));
            assert!(browser.read(cx).tab_slot(pinned).unwrap().pinned);
            pinned
        })
        .expect("ordinary close and pinned confirmation");
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
        assert!(browser.read(cx).tab_slot(pinned).unwrap().pinned);
        assert!(
            browser
                .read(cx)
                .locations
                .pinned_paths
                .contains(&directory.path().to_owned())
        );
        window.press("ctrl-w", cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
    })
    .expect("cancel preserves the pin");
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.press("enter", cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
        assert!(browser.read(cx).tab_slot(pinned).is_none());
        assert!(
            !browser
                .read(cx)
                .locations
                .pinned_paths
                .contains(&directory.path().to_owned())
        );
        assert_eq!(
            browser.read(cx).tabs.items.len(),
            1,
            "last close opens an ordinary Home tab"
        );
        for compact in [false, true] {
            let id = browser.read(cx).tabs.active_id;
            browser.update(cx, |browser, cx| {
                browser.sidebar.collapsed = compact;
                browser.toggle_pin(id, cx);
            });
            window.render_frame(cx);
            let action = if compact {
                "unpin-tab-corner"
            } else {
                "unpin-tab"
            };
            assert!(window.find((action, id)).visible());
            assert!(window.try_find(("close-tab", id)).is_none());
            window.click((action, id), cx);
            window.render_frame(cx);
            assert!(!window.has_active_dialog(cx));
            assert!(!browser.read(cx).tab_slot(id).unwrap().pinned);
            assert_eq!(browser.read(cx).tabs.items.len(), 1);
            window.press("ctrl-n", cx);
            window.render_frame(cx);
            browser.update(cx, |browser, cx| browser.switch_tab(id, window, cx));
            window.render_frame(cx);
            window.press("ctrl-w", cx);
            window.render_frame(cx);
            assert!(!window.has_active_dialog(cx));
            assert!(browser.read(cx).tab_slot(id).is_none());
        }
    })
    .expect("confirm and explicit unpin in both sidebar widths");
}
