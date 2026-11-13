use super::*;
use gpui_kit::{TestAppContext, test::TestWindowExt};

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        theme::init_default(cx);
        super::init(cx);
    });
}

#[gpui_kit::test]
fn about_renders_build_identity_and_closes_with_the_dialog_shortcut(cx: &mut TestAppContext) {
    init(cx);
    let about = cx.update(|cx| open_about(cx).expect("about"));
    cx.update_window(about.into(), |_, window, cx| {
        window.render_frame(cx);
        let version = format!(
            "{}: {}",
            gds::i18n::text("gds-about-version", cx),
            env!("CARGO_PKG_VERSION")
        );
        let tag = format!(
            "{}: {}",
            gds::i18n::text("gds-about-build-tag", cx),
            env!("GDS_BUILD_TAG")
        );
        assert!(window.find("about-version").visible());
        assert_eq!(window.find("about-version").label(), Some(version.as_str()));
        assert!(window.find("about-build-tag").visible());
        assert_eq!(window.find("about-build-tag").label(), Some(tag.as_str()));
        assert!(window.find("dialog-cancel").visible());
        window.press("alt-c", cx);
    })
    .expect("render build identity");
    assert!(cx.update_window(about.into(), |_, _, _| ()).is_err());
}

#[gpui_kit::test]
fn quit_is_immediate_idempotent_and_opens_no_confirmation(cx: &mut TestAppContext) {
    init(cx);
    cx.update(|cx| {
        // TestPlatform::quit is a no-op, so inspect the request without exiting.
        request_quit(cx);
        assert!(cx.has_global::<QuitPending>());
        assert!(cx.windows().is_empty());
        request_quit(cx);
        assert!(cx.windows().is_empty());
    });
}

fn request() -> Confirmation {
    Confirmation {
        title: "永久删除线程？".into(),
        message: "此操作不可撤销。".into(),
        confirm_label: "永久删除".into(),
        destructive: true,
    }
}

#[gpui_kit::test]
fn notices_do_not_replace_confirmations_or_open_windows(cx: &mut TestAppContext) {
    use std::{cell::Cell, rc::Rc};
    init(cx);
    let calls = Rc::new(Cell::new(0));
    let callback_calls = calls.clone();
    let confirmation = cx.update(|cx| {
        open_confirmation(
            request(),
            move |_| {
                callback_calls.set(callback_calls.get() + 1);
            },
            cx,
        )
        .expect("confirmation")
    });
    let again = cx.update(|cx| {
        open_confirmation(
            request(),
            |_| panic!("new request must not replace pending request"),
            cx,
        )
        .expect("focus pending confirmation")
    });
    assert_eq!(confirmation.window_id(), again.window_id());
    let about = cx.update(|cx| open_about(cx).expect("separate about"));
    cx.update(|cx| {
        crate::shared::lib::diagnostics::notify(
            Some(about.into()),
            crate::shared::lib::diagnostics::log::Level::Error,
            "归档失败",
            "请稍后重试",
            cx,
        );
    });
    cx.update(|cx| assert_eq!(cx.windows().len(), 2));
    assert_eq!(calls.get(), 0);
    cx.update_window(confirmation.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("application-message-body-copy", cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some(request().message)
        );
        window.click("message-confirm", cx);
    })
    .expect("confirm original request");
    assert_eq!(calls.get(), 1);
    assert!(cx.update_window(confirmation.into(), |_, _, _| ()).is_err());
    cx.update_window(about.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("about-window").visible());
    })
    .expect("unrelated about is intact");
}

#[gpui_kit::test]
fn quit_flushes_valid_settings_before_debounce_without_a_workspace(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("temporary appearance");
    let path = directory.path().join("settings.json");
    std::fs::write(&path, r#"{"appearance":{"theme_source":"builtin"}}"#).expect("test config");
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        crate::shared::config::init(Some(path.clone()), cx);
        theme::init(cx);
    });
    let settings = cx.update(|cx| settings::window::open(cx).expect("settings"));
    cx.update_window(settings.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("settings-fonts", cx);
        window.render_frame(cx);
        window.click("appearance-fallbacks", cx);
        window.input("Fixture Sans", cx);
    })
    .expect("edit before debounce");
    cx.update(request_quit);
    let saved: crate::shared::config::store::Settings =
        serde_json::from_slice(&std::fs::read(&path).expect("saved settings"))
            .expect("preferences");
    assert_eq!(
        saved.appearance.font_fallbacks,
        Some(vec!["Fixture Sans".to_owned()])
    );
}

use crate::features::navigation::open_about;
use crate::shared::lib::appearance as theme;
use crate::shared::ui::dialogs::{Confirmation, open_confirmation};
use gpui_kit::AppContext;

mod backend;
