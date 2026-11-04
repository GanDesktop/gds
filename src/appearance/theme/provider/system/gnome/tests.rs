use super::*;

fn roles() -> serde_json::Value {
    serde_json::json!({
        "theme_bg_color": [0.2, 0.3, 0.4, 1.0],
        "theme_fg_color": [0.8, 0.9, 0.7, 1.0],
        "theme_base_color": [0.1, 0.2, 0.3, 1.0],
        "theme_text_color": [0.9, 0.8, 0.7, 1.0],
        "theme_selected_bg_color": [0.6, 0.2, 0.5, 1.0],
        "theme_selected_fg_color": [0.2, 0.9, 0.8, 1.0]
    })
}

#[test]
fn imported_colors_and_pairs_are_preserved_without_an_adwaita_table() {
    let loaded = parse(
        &serde_json::to_vec(&roles()).expect("valid GTK fixture"),
        ModePreference::System,
    )
    .expect("valid GTK fixture");
    let color = |r, g, b| Hsla::from(Rgba { r, g, b, a: 1.0 });
    assert_eq!(loaded.mode, Mode::Dark);
    assert_eq!(loaded.palette.content, color(0.1, 0.2, 0.3));
    assert_eq!(loaded.palette.selection, color(0.6, 0.2, 0.5));
    assert_eq!(loaded.palette.selection_foreground, color(0.2, 0.9, 0.8));
    assert_eq!(loaded.palette.tooltip, loaded.palette.panel);
    assert_eq!(
        loaded.palette.tooltip_foreground,
        loaded.palette.panel_foreground
    );
    assert_eq!(loaded.palette.terminal.selection, loaded.palette.selection);
}

#[test]
fn incomplete_or_invalid_native_palettes_are_errors() {
    let mut missing = roles();
    missing
        .as_object_mut()
        .expect("valid GTK fixture")
        .remove("theme_selected_fg_color");
    assert!(
        parse(
            &serde_json::to_vec(&missing).expect("valid GTK fixture"),
            ModePreference::System
        )
        .is_err()
    );
    let mut invalid = roles();
    invalid["theme_base_color"] = serde_json::json!([1.5, 0.2, 0.3, 1.0]);
    assert!(
        parse(
            &serde_json::to_vec(&invalid).expect("valid GTK fixture"),
            ModePreference::System
        )
        .is_err()
    );
}
