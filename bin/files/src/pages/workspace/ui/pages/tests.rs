use super::super::{LaunchOptions, Places, configuration};
use super::{Browser, Page};
use gpui_kit::{AppContext, TestAppContext, component::Root, px, size, test::TestWindowExt};

#[gpui_kit::test]
fn help_returns_through_settings_without_changing_file_tabs(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("workspace directory");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("unpolled runtime");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("translations");
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
    let browser = browser.expect("browser");
    cx.update_window(handle.into(), |_, window, cx| {
        browser.read(cx).window.focus.clone().focus(window, cx);
        window.render_frame(cx);
        let active_tab = browser.read(cx).tabs.active_id;
        let tab_count = browser.read(cx).tabs.items.len();
        window.press("ctrl-,", cx);
        window.render_frame(cx);
        browser
            .read(cx)
            .settings
            .view
            .clone()
            .update(cx, |view, cx| {
                for _ in 0..3 {
                    view.cycle_page(false, window, cx);
                }
            });
        window.render_frame(cx);
        let settings_focus = window.focused(cx).expect("settings page focused");
        window.press("f1", cx);
        window.render_frame(cx);
        assert!(window.find("help-page").visible());
        window.press("f1", cx);
        window.press("ctrl-n", cx);
        assert_eq!(browser.read(cx).pages.len(), 2);
        assert_eq!(browser.read(cx).tabs.items.len(), tab_count);
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(browser.read(cx).pages.current() == Some(&Page::Settings));
        assert!(settings_focus.is_focused(window));
        window.press("alt-left", cx);
        window.render_frame(cx);
        assert!(browser.read(cx).pages.is_empty());
        assert_eq!(browser.read(cx).tabs.active_id, active_tab);
        assert!(browser.read(cx).window.focus.is_focused(window));
        window.press("f1", cx);
        window.render_frame(cx);
        window.press("ctrl-w", cx);
        window.render_frame(cx);
        assert!(browser.read(cx).pages.is_empty());
        assert_eq!(browser.read(cx).tabs.items.len(), tab_count);
    })
    .expect("help navigation");
    cx.update(|cx| assert_eq!(cx.windows().len(), 1));
}
