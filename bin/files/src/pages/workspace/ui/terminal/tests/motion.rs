use crate::pages::workspace::ui::{
    Browser, LaunchOptions, Places, configuration, motion::DURATION,
};
use gpui_kit::{AppContext, TestAppContext, component::Root, px, size, test::TestWindowExt};

#[gpui_kit::test]
fn maximizing_and_restoring_terminal_preserves_the_split(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("workspace directory");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("unpolled runtime");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("translations");
    });
    let mut browser = None;
    let handle = cx.open_window(size(px(1100.), px(800.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut preferences = configuration::Preferences::default();
            preferences.terminal.shell = Some(directory.path().join("missing-test-shell"));
            Browser::new(
                LaunchOptions {
                    runtime: runtime.handle().clone(),
                    places: Ok(Places::default()),
                    start: directory.path().to_owned(),
                    home: directory.path().to_owned(),
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
    let browser = browser.expect("browser");
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        browser.update(cx, |browser, cx| browser.toggle_terminal(window, cx));
        window.render_frame(cx);
    })
    .expect("show terminal");
    cx.run_until_parked();
    cx.executor().advance_clock(DURATION);
    let (normal, preferred) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            let normal = window.find("files-terminal-dock").bounds().size.height;
            let preferred = browser.read(cx).terminal.split.read(cx).height();
            window.click("files-terminal-maximize", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("files-terminal-dock").bounds().size.height,
                normal
            );
            (normal, preferred)
        })
        .expect("start maximizing");
    cx.executor().advance_clock(DURATION / 2);
    let middle = cx
        .update_window(handle.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            window.find("files-terminal-dock").bounds().size.height
        })
        .expect("sample maximizing");
    assert!(middle > normal);
    cx.executor().advance_clock(DURATION);
    let full = cx
        .update_window(handle.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            let full = window.find("files-terminal-dock").bounds().size.height;
            assert!(full > middle);
            assert_eq!(browser.read(cx).terminal.split.read(cx).height(), preferred);
            window.click("files-terminal-maximize", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("files-terminal-dock").bounds().size.height,
                full
            );
            full
        })
        .expect("start restoring");
    cx.executor().advance_clock(DURATION / 2);
    cx.update_window(handle.into(), |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        let restoring = window.find("files-terminal-dock").bounds().size.height;
        assert!(restoring < full && restoring > normal);
    })
    .expect("sample restoring");
    cx.executor().advance_clock(DURATION);
    cx.update_window(handle.into(), |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("files-terminal-dock").bounds().size.height,
            normal
        );
        assert_eq!(browser.read(cx).terminal.split.read(cx).height(), preferred);
    })
    .expect("restore preferred split");
}

#[gpui_kit::test]
fn disabling_terminal_hides_its_controls_and_prevents_opening(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("workspace directory");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::features::file_operations::commands::register(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).unwrap();
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
                    preferences: configuration::Preferences {
                        terminal_enabled: Some(false),
                        ..Default::default()
                    },
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
    cx.update_window(handle.into(), |_, window, cx| {
        browser.read(cx).window.focus.clone().focus(window, cx);
        window.render_frame(cx);
        assert!(gpui_kit::base::test_support::find(window, &[], &"terminal".into()).is_none());
        window.press("ctrl-`", cx);
        window.render_frame(cx);
        assert!(!browser.read(cx).terminal_visible());
        assert!(browser.read(cx).terminal.dock.is_none());
        assert!(
            gpui_kit::base::test_support::find(window, &[], &"files-terminal-dock".into())
                .is_none()
        );
    })
    .expect("disabled terminal stays absent");
}
