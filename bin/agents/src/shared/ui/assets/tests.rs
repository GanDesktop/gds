use super::*;
use gpui_kit::assets::IconName;

#[test]
fn terminal_management_icons_are_available_to_the_application() {
    for icon in [
        IconName::Terminal,
        IconName::List,
        IconName::X,
        IconName::Plus,
    ] {
        let path = icon.path();
        let asset = Assets
            .load(&path)
            .unwrap_or_else(|error| panic!("terminal action asset {path}: {error}"))
            .unwrap_or_else(|| panic!("terminal action asset {path} is missing"));
        assert!(!asset.is_empty(), "terminal action asset {path} is empty");
    }
}

#[test]
fn clear_list_icon_is_bundled() {
    use gpui_kit::AssetSource;
    assert!(
        crate::shared::ui::assets::Assets
            .load("icons/list-x.svg")
            .expect("icon")
            .is_some()
    );
}
