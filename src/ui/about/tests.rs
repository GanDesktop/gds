use super::{About, Application, HEIGHT, WIDTH};
use crate::app::AppExt as _;
use gpui_kit::{
    AppContext as _, ParentElement as _, Styled as _, TestAppContext,
    component::{ActiveTheme as _, Root, WindowExt, dialog::DialogEntrance},
    div, px, size,
    test::TestWindowExt,
};

#[gpui_kit::test]
fn cargo_identity_can_be_copied_and_information_pages_return_to_the_overview(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds()
            .i18n()
            .install(crate::i18n::Language::English)
            .expect("translations");
        cx.gds().ui().register_dialogs();
        cx.gds()
            .appearance()
            .theme()
            .apply(&crate::appearance::theme::Appearance {
                source: crate::appearance::theme::Source::Builtin,
                content_opacity: 0.35,
                ..Default::default()
            })
            .expect("GDS appearance");
    });
    let app = Application::cargo("gds-files");
    assert_eq!(app.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(app.build_tag, env!("GDS_BUILD_TAG"));
    let window = cx.open_window(size(px(WIDTH), px(HEIGHT)), |window, cx| {
        let about = cx.new(|cx| About::new(app, cx));
        Root::new(about, window, cx)
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("about-version", cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some(app.version.to_owned())
        );
        window.click("about-build-tag", cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some(app.build_tag.to_owned())
        );

        window.click("about-credits", cx);
        window.render_frame(cx);
        assert!(window.find("about-back").visible());
        window.click("about-back", cx);
        window.render_frame(cx);
        window.click("about-license", cx);
        window.render_frame(cx);
        assert!(window.find("about-back").visible());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.find("about-version").visible());
    })
    .expect("About interaction");
}

#[gpui_kit::test]
fn escape_returns_from_license_and_the_close_shortcut_dismisses_the_dialog(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds()
            .i18n()
            .install(crate::i18n::Language::English)
            .expect("translations");
        cx.gds().ui().register_dialogs();
        cx.gds()
            .appearance()
            .theme()
            .apply(&crate::appearance::theme::Appearance {
                source: crate::appearance::theme::Source::Builtin,
                content_opacity: 0.35,
                ..Default::default()
            })
            .expect("GDS appearance");
    });
    let window = cx.open_window(size(px(800.), px(700.)), |window, cx| {
        let view = cx.new(|_| gpui_kit::Empty);
        Root::new(view, window, cx)
    });
    cx.update_window(window.into(), |_, window, cx| {
        let windows = cx.windows().len();
        window.render_frame(cx);
        let background: gpui_kit::Background = cx.theme().tokens.background.clone().into();
        assert_eq!(cx.theme().background.a, 1.0);
        assert!(
            window
                .painted_quads()
                .iter()
                .all(|quad| quad.background != background),
            "an empty window must not paint the opaque theme background under its materials",
        );
        let about = cx.new(|cx| About::new(Application::cargo("gds-files"), cx));
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let footer = crate::ui::dialog::DialogFooter::default().child(
                crate::ui::dialog::DialogDecision::new(crate::ui::dialog::DialogCommand::Cancel)
                    .label(crate::i18n::text("close", cx)),
            );
            dialog
                .entrance(DialogEntrance::None)
                .footer(footer)
                .child(div().h(px(HEIGHT)).child(about.clone()))
        });
        assert_eq!(cx.windows().len(), windows, "About is an in-window dialog");
        window.render_frame(cx);
        for mode in [
            crate::appearance::theme::ModePreference::Light,
            crate::appearance::theme::ModePreference::Dark,
        ] {
            cx.gds()
                .appearance()
                .theme()
                .apply(&crate::appearance::theme::Appearance {
                    source: crate::appearance::theme::Source::Builtin,
                    mode,
                    content_opacity: 0.35,
                    ..Default::default()
                })
                .expect("switch GDS appearance");
            window.render_frame(cx);
            let surface = window.find("surface").bounds();
            let background: gpui_kit::Background = cx.theme().tokens.background.clone().into();
            let scale = window.scale_factor();
            assert_eq!(cx.theme().background.a, 1.0);
            assert!(
                window.painted_quads().iter().any(|quad| {
                    quad.background == background
                        && quad
                            .bounds
                            .map(|value| px(value.0 / scale))
                            .contains(&surface.center())
                }),
                "the native dialog must paint an opaque surface after a theme switch"
            );
        }
        window.click("about-license", cx);
        window.render_frame(cx);
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        assert!(window.find("about-version").visible());
        window.press("alt-c", cx);
        assert!(!window.has_active_dialog(cx));
    })
    .expect("About dialog keyboard navigation");
}
