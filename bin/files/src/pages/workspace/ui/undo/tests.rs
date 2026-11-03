use super::super::{Browser, Command, LaunchOptions, Places, configuration};
use gpui_kit::{
    AppContext, Entity, TestAppContext,
    component::{Root, WindowExt},
    px, size,
    test::TestWindowExt,
};
use std::time::{Duration, Instant};

#[gpui_kit::test]
fn rename_undo_redo_use_shortcuts_and_menu_without_intercepting_text(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = tempfile::tempdir().expect("undo UI fixture");
    let before = root.path().join("before.txt");
    let after = root.path().join("after.txt");
    std::fs::write(&before, "contents").unwrap();
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
    let browser = browser.unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        browser.read(cx).window.focus.clone().focus(window, cx);
        window.render_frame(cx);
        window.click("application-menu", cx);
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        for label in ["Undo", "Redo"] {
            window.within("popup-menu").click(label, cx);
            assert!(
                browser.read(cx).menus.application_open,
                "disabled history keeps the menu open"
            );
            assert!(!browser.read(cx).operations.in_progress);
        }
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!browser.read(cx).menus.application_open);
        crate::shared::lib::clipboard::clear(cx);
        window.render_frame(cx);
        window.right_click(("file-name", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        window.within("popup-menu").click("Paste", cx);
        assert!(
            !browser.read(cx).operations.in_progress,
            "empty clipboard cannot start a paste"
        );
        assert!(
            window.try_find("popup-menu").is_some(),
            "disabled paste keeps the menu open"
        );
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        browser.read(cx).window.focus.clone().focus(window, cx);
        window.render_frame(cx);
        window.press("down", cx);
        assert_eq!(browser.read(cx).selected_visible_paths(), [before.clone()]);
        window.press("f2", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "rename dialog opened");
        window.press("ctrl-a", cx);
        window.input("after.txt", cx);
        window.press("enter", cx);
    })
    .unwrap();
    wait(cx, &browser, || after.exists());
    for (stroke, expected) in [("ctrl-z", &before), ("ctrl-shift-z", &after)] {
        cx.update_window(handle.into(), |_, window, cx| {
            browser.read(cx).window.focus.clone().focus(window, cx);
            window.render_frame(cx);
            window.press(stroke, cx);
        })
        .unwrap();
        wait(cx, &browser, || expected.exists());
        assert_eq!(std::fs::read_to_string(expected).unwrap(), "contents");
    }
    cx.update_window(handle.into(), |_, window, cx| {
        window.press("ctrl-f", cx);
        window.input("edited", cx);
        window.press("ctrl-z", cx);
        assert!(
            browser
                .read(cx)
                .toolbar
                .filter
                .input
                .read(cx)
                .value()
                .is_empty()
        );
        assert!(after.exists(), "text undo must leave files alone");
        assert!(!browser.read(cx).operations.in_progress);
        browser.read(cx).window.focus.clone().focus(window, cx);
        window.render_frame(cx);
        window.click("application-menu", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.within("popup-menu").click(0usize, cx);
    })
    .unwrap();
    wait(cx, &browser, || before.exists());
    cx.update(|cx| {
        assert!(!browser.read(cx).history_available(Command::Undo, cx));
        assert!(browser.read(cx).history_available(Command::Redo, cx));
    });
}

#[track_caller]
fn wait(cx: &mut TestAppContext, browser: &Entity<Browser>, ready: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if ready() && cx.update(|cx| !browser.read(cx).operations.in_progress) {
            break;
        }
        assert!(Instant::now() < deadline, "file operation completed");
        std::thread::sleep(Duration::from_millis(10));
    }
}
