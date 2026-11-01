use super::*;
use crate::shared::config;
use gpui_kit::{AppContext, TestAppContext, test::TestWindowExt};
use std::{cell::Cell, rc::Rc};

#[gpui_kit::test]
fn opting_out_is_persisted_only_when_deletion_is_confirmed(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("temporary preferences");
    let path = directory.path().join("settings.json");
    std::fs::write(&path, r#"{"appearance":{"theme_source":"builtin"}}"#)
        .expect("initial preferences");
    cx.update(|cx| {
        gpui_kit::init(cx);
        config::init(Some(path.clone()), cx);
        appearance::init(cx);
    });
    let calls = Rc::new(Cell::new(0));
    let open = |cx: &mut TestAppContext| {
        let calls = calls.clone();
        cx.update(|cx| {
            confirm(
                Confirmation {
                    title: "Delete session?".into(),
                    message: "This removes the selected session.".into(),
                    confirm_label: "Delete".into(),
                    destructive: true,
                },
                move |_| calls.set(calls.get() + 1),
                cx,
            )
            .expect("confirmation window");
            cx.windows().last().copied().expect("open confirmation")
        })
    };
    let cancelled = open(cx);
    cx.update_window(cancelled, |_, window, cx| {
        window.render_frame(cx);
        window.click("delete-dont-ask", cx);
        window.click("message-close", cx);
    })
    .expect("cancel deletion");
    assert_eq!(calls.get(), 0);
    cx.update(|cx| {
        assert_eq!(
            config::store(cx)
                .load()
                .expect("unchanged preferences")
                .settings
                .appearance
                .confirm_session_delete,
            None
        )
    });

    let confirmed = open(cx);
    cx.update_window(confirmed, |_, window, cx| {
        window.render_frame(cx);
        window.click("delete-dont-ask", cx);
        window.click("message-confirm", cx);
    })
    .expect("confirm deletion");
    assert_eq!(calls.get(), 1);
    cx.update(|cx| {
        assert_eq!(
            config::store(cx)
                .load()
                .expect("saved preferences")
                .settings
                .appearance
                .confirm_session_delete,
            Some(false)
        );
        let calls = calls.clone();
        confirm(
            Confirmation {
                title: "Delete".into(),
                message: String::new(),
                confirm_label: "Delete".into(),
                destructive: true,
            },
            move |_| calls.set(calls.get() + 1),
            cx,
        )
        .expect("remembered opt-out");
        assert!(cx.windows().is_empty());
    });
    assert_eq!(calls.get(), 2);
}
