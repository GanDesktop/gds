use super::*;
use gpui_kit::{TestAppContext, component::slider::SliderValue, px, size, test::TestWindowExt};

#[gpui_kit::test]
fn builtin_palette_selection_applies_persists_and_resets_independently(cx: &mut TestAppContext) {
    use gds::appearance::theme::{BuiltinTheme, Colors};
    let directory = tempfile::tempdir().expect("isolated appearance");
    let path = directory.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"appearance":{"theme_source":"builtin","theme_mode":"light"}}"#,
    )
    .expect("legacy built-in setting");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::config::init(Some(path.clone()), cx);
        theme::init(cx);
    });
    let mut page = None;
    let window = cx.open_window(size(px(1000.), px(850.)), |window, cx| {
        let editor = theme::Editor::current(cx);
        let view = cx.new(|cx| AppearancePage::new(editor, window, cx));
        page = Some(view.clone());
        Root::new(view, window, cx)
    });
    let page = page.expect("appearance page");
    let original = cx.update(|cx| Colors::current(cx));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let select = page.read(cx).form.builtin.clone();
        select.update(cx, |select, cx| {
            select.set_selected_value(&BuiltinTheme::Neon, window, cx);
            cx.emit(SelectEvent::Confirm(Some(BuiltinTheme::Neon)));
        });
    })
    .expect("select embedded palette");
    let store = crate::shared::config::Store::at(path);
    let saved = store.load().expect("saved palette").settings.appearance;
    assert_eq!(saved.builtin_theme, BuiltinTheme::Neon);
    assert_eq!(saved.theme_mode, theme::preferences::ThemeMode::Light);
    cx.update_window(window.into(), |_, window, cx| {
        assert_ne!(Colors::current(cx), original);
        window.render_frame(cx);
        window.click("appearance-builtin-theme-default", cx);
        assert_eq!(Colors::current(cx), original);
    })
    .expect("reset embedded palette");
    let saved = store.load().expect("saved reset").settings.appearance;
    assert_eq!(saved.builtin_theme, BuiltinTheme::Bamboo);
    assert_eq!(saved.theme_mode, theme::preferences::ThemeMode::Light);
}

#[gpui_kit::test]
fn background_opacities_apply_save_and_reset_independently(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("isolated appearance");
    let path = directory.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"appearance":{"theme_source":"builtin","content_opacity":0.74,"sidebar_opacity":0.39}}"#,
    )
    .expect("initial appearance");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::config::init(Some(path.clone()), cx);
        theme::init(cx);
    });
    let mut page = None;
    let window = cx.open_window(size(px(1000.), px(850.)), |window, cx| {
        let editor = theme::Editor::current(cx);
        let view = cx.new(|cx| AppearancePage::new(editor, window, cx));
        page = Some(view.clone());
        Root::new(view, window, cx)
    });
    let page = page.expect("appearance page");
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("appearance-opacity-row").visible());
        assert!(window.find("appearance-sidebar-opacity-row").visible());
        page.update(cx, |page, cx| {
            page.apply_number(Field::SidebarOpacity, SliderValue::Single(26.), window, cx);
        });
        assert_eq!(gds::appearance::theme::content_background(cx).a, 0.74);
        assert_eq!(gds::appearance::theme::sidebar_background(cx).a, 0.26);
        page.update(cx, |page, cx| {
            page.apply_number(Field::Opacity, SliderValue::Single(62.), window, cx);
        });
    })
    .expect("apply both controls");
    let store = crate::shared::config::Store::at(path);
    let saved = store.load().expect("saved controls").settings.appearance;
    assert_eq!(saved.content_opacity, Some(0.62));
    assert_eq!(saved.sidebar_opacity, Some(0.26));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("appearance-sidebar-opacity-default", cx);
        assert_eq!(gds::appearance::theme::content_background(cx).a, 0.62);
        assert_eq!(
            gds::appearance::theme::sidebar_background(cx).a,
            gds::appearance::tokens::SurfaceOpacity::default().sidebar
        );
    })
    .expect("reset sidebar only");
    let saved = store.load().expect("sidebar reset").settings.appearance;
    assert_eq!(saved.content_opacity, Some(0.62));
    assert_eq!(saved.sidebar_opacity, None);
    cx.update_window(window.into(), |_, window, cx| {
        window.click("appearance-opacity-default", cx);
        assert_eq!(
            gds::appearance::theme::content_background(cx).a,
            gds::appearance::tokens::SurfaceOpacity::default().content
        );
    })
    .expect("reset main background");
    let saved = store.load().expect("main reset").settings.appearance;
    assert_eq!(saved.content_opacity, None);
    assert_eq!(saved.sidebar_opacity, None);
}
