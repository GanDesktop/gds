use super::*;

#[gpui_kit::test]
fn selected_store_is_available_without_theme_and_survives_theme_initialization(
    cx: &mut gpui_kit::TestAppContext,
) {
    let directory = tempfile::tempdir().expect("isolated configuration");
    let path = directory.path().join("selected.json");
    cx.update(|cx| {
        init(Some(path.clone()), cx);
        let selected = store(cx);
        assert_eq!(selected.path(), path);
        let mut document = selected.load().expect("default configuration");
        let appearance = crate::shared::config::appearance::Appearance {
            ui_font_size: Some(17.0),
            ..Default::default()
        };
        selected
            .save_appearance(&mut document, appearance.clone())
            .expect("configuration writes need no theme or settings page");

        gpui_kit::init(cx);
        crate::shared::lib::appearance::init(cx);
        assert_eq!(store(cx).path(), path);
        assert_eq!(
            store(cx).load().expect("same store").settings.appearance,
            appearance
        );
        let editor = crate::shared::lib::appearance::Editor::current(cx);
        assert_eq!(editor.appearance_path(), path);
        assert_eq!(
            editor
                .load()
                .expect("appearance from selected store")
                .preferences,
            document.settings.appearance
        );
    });
}
