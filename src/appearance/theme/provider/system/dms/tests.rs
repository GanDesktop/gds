use super as dms;
use crate::appearance::theme::{Error, Mode, ModePreference, provider::color::rgb};
use std::path::Path;
use zgui::canvas::zgui_color::Color;

fn color(value: u32) -> Color {
    rgb(value)
}

fn dms_export(active_mode: &str) -> String {
    serde_json::json!({
        "mode": active_mode,
        "colors": {
            "dark": {"surface":"#101010", "on_surface":"#eeeeee", "primary":"#888888", "on_primary":"#111111"},
            "light": {"surface":"#abcdef", "on_surface":"#123456", "primary":"#654321", "on_primary":"#fedcba"}
        }
    }).to_string()
}

fn terminal_export(active: &str) -> serde_json::Value {
    let mut export: serde_json::Value = serde_json::from_str(&dms_export(active)).expect("fixture");
    for index in 0..16 {
        export["dank16"][format!("color{index}")] = serde_json::json!({
            "dark": format!("#{:06x}", 0x102030 + index),
            "light": format!("#{:06x}", 0xa0b0c0 + index),
            "default": "#ff00ff"
        });
    }
    export
}

#[test]
fn terminal_ansi_uses_all_sixteen_colors_of_the_resolved_mode() {
    for (active, preference, base) in [
        ("light", ModePreference::System, 0xa0b0c0),
        ("dark", ModePreference::System, 0x102030),
        ("light", ModePreference::Dark, 0x102030),
        ("dark", ModePreference::Light, 0xa0b0c0),
    ] {
        let parsed = dms::parse(
            &terminal_export(active).to_string(),
            Path::new("fixture.json"),
            preference,
        )
        .expect("complete terminal export");
        let roles = parsed.palette.terminal;
        for (index, actual) in roles.normal.into_iter().chain(roles.bright).enumerate() {
            assert_eq!(actual, color(base + index as u32));
        }
        assert_eq!(roles.background, parsed.palette.content);
        assert_eq!(roles.cursor_text, roles.background);
        assert_eq!(roles.selection_text, parsed.palette.selection_foreground);
    }
}

#[test]
fn absent_terminal_export_uses_complete_mode_appropriate_fallback() {
    let dark = dms::parse(
        &dms_export("dark"),
        Path::new("fixture.json"),
        ModePreference::System,
    )
    .expect("legacy export");
    let light = dms::parse(
        &dms_export("light"),
        Path::new("fixture.json"),
        ModePreference::System,
    )
    .expect("legacy export");
    assert_ne!(dark.palette.terminal.normal, light.palette.terminal.normal);
    assert_eq!(dark.palette.terminal.background, dark.palette.content);
    assert_eq!(light.palette.terminal.foreground, light.palette.foreground);
}

#[test]
fn present_terminal_export_never_silently_falls_back_or_uses_default() {
    for invalid in [
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!({"color0": {"default": "#ffffff"}}),
    ] {
        let mut export = terminal_export("dark");
        export["dank16"] = invalid;
        assert!(
            dms::parse(
                &export.to_string(),
                Path::new("fixture.json"),
                ModePreference::Dark
            )
            .is_err()
        );
    }
    for invalid in [
        serde_json::json!("not-a-color"),
        serde_json::json!(42),
        serde_json::Value::Null,
    ] {
        let mut export = terminal_export("dark");
        export["dank16"]["color15"]["dark"] = invalid;
        assert!(
            dms::parse(
                &export.to_string(),
                Path::new("fixture.json"),
                ModePreference::Dark
            )
            .is_err()
        );
    }
}

#[test]
fn dms_system_mode_selects_the_exported_active_scheme() {
    for (active, expected, surface) in [
        ("light", Mode::Light, 0xabcdef),
        ("dark", Mode::Dark, 0x101010),
    ] {
        let parsed = dms::parse(
            &dms_export(active),
            Path::new("dms-colors.json"),
            ModePreference::System,
        )
        .expect("active DMS scheme exists");
        assert_eq!(parsed.mode, expected);
        assert_eq!(parsed.palette.content, color(surface));
    }
}

#[test]
fn dms_active_mode_is_validated_only_when_following_system() {
    for metadata in [
        None,
        Some(serde_json::json!("unknown")),
        Some(serde_json::json!(42)),
    ] {
        let mut export: serde_json::Value =
            serde_json::from_str(&dms_export("light")).expect("valid fixture");
        let object = export.as_object_mut().expect("DMS export is an object");
        if let Some(value) = metadata {
            object.insert("mode".into(), value);
        } else {
            object.remove("mode");
        }
        let source = export.to_string();
        assert!(matches!(
            dms::parse(&source, Path::new("dms-colors.json"), ModePreference::System),
            Err(Error::Invalid { field, .. }) if field == "mode"
        ));
        let manual = dms::parse(&source, Path::new("dms-colors.json"), ModePreference::Dark)
            .expect("unrelated active-mode metadata must not block an explicit choice");
        assert_eq!(manual.mode, Mode::Dark);
    }
}

#[test]
fn dms_selects_dark_when_the_export_is_active_light() {
    let parsed = dms::parse(
        &dms_export("light"),
        Path::new("dms-colors.json"),
        ModePreference::Dark,
    )
    .expect("requested dark scheme exists");
    assert_eq!(parsed.mode, Mode::Dark);
    assert_eq!(parsed.palette.content, color(0x101010));
    assert_eq!(parsed.palette.foreground, color(0xeeeeee));
    assert_eq!(parsed.palette.selection, color(0x888888));
    assert_eq!(parsed.palette.selection_foreground, color(0x111111));
}

#[test]
fn dms_selects_light_when_the_export_is_active_dark() {
    let parsed = dms::parse(
        &dms_export("dark"),
        Path::new("dms-colors.json"),
        ModePreference::Light,
    )
    .expect("requested light scheme exists");
    assert_eq!(parsed.mode, Mode::Light);
    assert_eq!(parsed.palette.content, color(0xabcdef));
    assert_eq!(parsed.palette.foreground, color(0x123456));
    assert_eq!(parsed.palette.selection, color(0x654321));
    assert_eq!(parsed.palette.selection_foreground, color(0xfedcba));
}

#[test]
fn dms_preserves_surface_levels_boundaries_and_paired_selection_in_native_components() {
    let roles = serde_json::json!({
        "surface": "#101010", "on_surface": "#eeeeee",
        "surface_container_low": "#181818", "surface_container": "#202020",
        "surface_container_high": "#282828", "on_surface_variant": "#bbbbbb",
        "outline": "#888888", "outline_variant": "#444444", "shadow": "#010203",
        "primary": "#aabbcc", "on_primary": "#112233",
        "primary_container": "#345678", "on_primary_container": "#fedcba"
    });
    let source = serde_json::json!({"mode": "dark", "colors": {"dark": roles}}).to_string();
    let parsed = dms::parse(&source, Path::new("dms-colors.json"), ModePreference::Dark)
        .expect("complete Material color roles");
    assert_eq!(parsed.palette.sidebar, color(0x181818));
    assert_eq!(parsed.palette.surface, color(0x202020));
    assert_eq!(parsed.palette.panel, color(0x282828));
    assert_eq!(parsed.palette.panel_foreground, color(0xeeeeee));
    assert_eq!(parsed.palette.shadow, color(0x010203));
    let tokens = parsed.palette.tokens_theme(
        Mode::Dark,
        &crate::appearance::typography::Preferences::default()
            .resolve()
            .expect("default typography"),
        crate::appearance::motion::Policy::default(),
        true,
        crate::appearance::theme::Contrast::Normal,
    );
    assert_eq!(tokens.color.border, "#444444");
    assert_eq!(tokens.color.input, "#888888");
    assert_eq!(tokens.color.sidebar_accent, "#345678");
    assert_eq!(tokens.color.sidebar_accent_foreground, "#fedcba");
    assert_eq!(tokens.color.accent, tokens.color.sidebar_accent);
    assert_eq!(
        tokens.color.accent_foreground,
        tokens.color.sidebar_accent_foreground
    );
    assert_eq!(tokens.color.popover, "#282828");
}

#[test]
fn dms_missing_optional_surface_roles_use_the_imported_palette() {
    let parsed = dms::parse(
        &dms_export("dark"),
        Path::new("dms-colors.json"),
        ModePreference::Dark,
    )
    .expect("minimal export");
    let colors = parsed.palette;
    assert_eq!(colors.surface, colors.content);
    assert_eq!(colors.panel, colors.content);
    assert_eq!(colors.panel_foreground, colors.foreground);
    assert_eq!(colors.outline, colors.border);
}

#[test]
fn dms_missing_requested_scheme_does_not_use_the_available_other_scheme() {
    for (requested, source, missing_field) in [
        (
            ModePreference::Dark,
            r##"{"mode":"light","colors":{"light":{"surface":"#ffffff","on_surface":"#000000","primary":"#123456","on_primary":"#ffffff"}}}"##,
            "colors.dark",
        ),
        (
            ModePreference::Light,
            r##"{"mode":"dark","colors":{"dark":{"surface":"#000000","on_surface":"#ffffff","primary":"#abcdef","on_primary":"#000000"}}}"##,
            "colors.light",
        ),
        (
            ModePreference::System,
            r##"{"mode":"dark","colors":{"light":{"surface":"#ffffff","on_surface":"#000000","primary":"#123456","on_primary":"#ffffff"}}}"##,
            "colors.dark",
        ),
    ] {
        assert!(matches!(
            dms::parse(source, Path::new("dms-colors.json"), requested),
            Err(Error::Invalid { field, .. }) if field == missing_field
        ));
    }
}

#[test]
fn dms_rejects_missing_roles_and_incomplete_color_pairs() {
    for source in [
        r##"{"mode":"dark","colors":{"light":{}}}"##,
        r##"{"mode":"dark","colors":{"dark":{"surface":"#111111"}}}"##,
        r##"{"mode":"dark","colors":{"dark":{"surface":"#111111","on_surface":"#eeeeee","primary":"#abcdef","on_primary":"#123456","primary_container":"#222222"}}}"##,
        r##"{"mode":"dark","colors":{"dark":{"surface":"#111111","on_surface":"#eeeeee","primary":"#abcdef","on_primary":"#123456","secondary_container":"#222222"}}}"##,
        r##"{"mode":"dark","colors":{"dark":{"surface":"#111111","on_surface":"#eeeeee","primary":"#abcdef","on_primary":"#123456","tertiary":"#222222"}}}"##,
        r##"{"mode":"dark","colors":{"dark":{"surface":"#111111","on_surface":"#eeeeee","primary":"#abcdef","on_primary":"#123456","error_container":"#222222"}}}"##,
        r##"{"mode":"dark","colors":{"dark":{"surface":"#111111","on_surface":"#eeeeee","primary":"#abcdef","on_primary":"#123456","inverse_surface":"#222222"}}}"##,
        r##"{"mode":"dark","colors":{"dark":{"surface":"not-a-color","on_surface":"#eeeeee","primary":"#abcdef","on_primary":"#123456"}}}"##,
    ] {
        assert!(dms::parse(source, Path::new("dms-colors.json"), ModePreference::Dark).is_err());
    }
}

#[test]
fn dms_full_material3_export_maps_all_50_roles_and_contrasting_neutral_control() {
    let source = r##"{
        "mode": "light",
        "colors": {
            "light": {
                "background": "#fff8f6",
                "on_background": "#231917",
                "surface": "#fff8f6",
                "on_surface": "#231917",
                "surface_dim": "#e8d6d1",
                "surface_bright": "#fff8f6",
                "surface_container_lowest": "#ffffff",
                "surface_container_low": "#fff1ed",
                "surface_container": "#fff8f6",
                "surface_container_high": "#fceae5",
                "surface_container_highest": "#f7e4df",
                "surface_variant": "#f5ded7",
                "on_surface_variant": "#53433e",
                "surface_tint": "#8f4c34",
                "inverse_surface": "#392e2b",
                "inverse_on_surface": "#ffede8",
                "inverse_primary": "#ffb59c",
                "outline": "#85736e",
                "outline_variant": "#d8c2bb",
                "shadow": "#000000",
                "scrim": "#010101",
                "source_color": "#ecccc2",
                "primary": "#8f4c34",
                "on_primary": "#ffffff",
                "primary_container": "#ffdbcf",
                "on_primary_container": "#390c00",
                "primary_fixed": "#ffdbcf",
                "primary_fixed_dim": "#ffb59c",
                "on_primary_fixed": "#390c00",
                "on_primary_fixed_variant": "#72351f",
                "secondary": "#77574c",
                "on_secondary": "#ffffff",
                "secondary_container": "#ffdbcf",
                "on_secondary_container": "#2c160e",
                "secondary_fixed": "#ffdbcf",
                "secondary_fixed_dim": "#e7bdb0",
                "on_secondary_fixed": "#2c160e",
                "on_secondary_fixed_variant": "#5d4036",
                "tertiary": "#6a5e2f",
                "on_tertiary": "#ffffff",
                "tertiary_container": "#f3e2a7",
                "on_tertiary_container": "#221b00",
                "tertiary_fixed": "#f3e2a7",
                "tertiary_fixed_dim": "#d6c68d",
                "on_tertiary_fixed": "#221b00",
                "on_tertiary_fixed_variant": "#51461a",
                "error": "#ba1a1a",
                "on_error": "#ffffff",
                "error_container": "#ffdad6",
                "on_error_container": "#410002"
            },
            "dark": {
                "background": "#1a110f",
                "on_background": "#f1dfd9",
                "surface": "#1a110f",
                "on_surface": "#f1dfd9",
                "surface_dim": "#1a110f",
                "surface_bright": "#423733",
                "surface_container_lowest": "#140c0a",
                "surface_container_low": "#231917",
                "surface_container": "#1a110f",
                "surface_container_high": "#271d1a",
                "surface_container_highest": "#322824",
                "surface_variant": "#53433e",
                "on_surface_variant": "#d8c2bb",
                "surface_tint": "#ffb59c",
                "inverse_surface": "#f1dfd9",
                "inverse_on_surface": "#392e2b",
                "inverse_primary": "#8f4c34",
                "outline": "#a08d87",
                "outline_variant": "#53433e",
                "shadow": "#000000",
                "scrim": "#020202",
                "source_color": "#ecccc2",
                "primary": "#ffb59c",
                "on_primary": "#55200b",
                "primary_container": "#72351f",
                "on_primary_container": "#ffdbcf",
                "primary_fixed": "#ffdbcf",
                "primary_fixed_dim": "#ffb59c",
                "on_primary_fixed": "#390c00",
                "on_primary_fixed_variant": "#72351f",
                "secondary": "#e7bdb0",
                "on_secondary": "#442a21",
                "secondary_container": "#5d4036",
                "on_secondary_container": "#ffdbcf",
                "secondary_fixed": "#ffdbcf",
                "secondary_fixed_dim": "#e7bdb0",
                "on_secondary_fixed": "#2c160e",
                "on_secondary_fixed_variant": "#5d4036",
                "tertiary": "#d6c68d",
                "on_tertiary": "#393005",
                "tertiary_container": "#51461a",
                "on_tertiary_container": "#f3e2a7",
                "tertiary_fixed": "#f3e2a7",
                "tertiary_fixed_dim": "#d6c68d",
                "on_tertiary_fixed": "#221b00",
                "on_tertiary_fixed_variant": "#51461a",
                "error": "#ffb4ab",
                "on_error": "#690005",
                "error_container": "#93000a",
                "on_error_container": "#ffdad6"
            }
        }
    }"##;

    let typo = crate::appearance::typography::Preferences::default()
        .resolve()
        .expect("default typography");
    let motion = crate::appearance::motion::Policy::default();

    let light = dms::parse(source, Path::new("dms-colors.json"), ModePreference::Light)
        .expect("full DMS light scheme");
    assert_eq!(light.palette.surface_container_lowest, color(0xffffff));
    assert_eq!(light.palette.surface_container_highest, color(0xf7e4df));
    assert_eq!(light.palette.panel, color(0xfceae5));
    // Light neutral control stands above panel (#fceae5) and content (#fff8f6) using lowest (#ffffff).
    assert_eq!(light.palette.control, color(0xffffff));
    assert_ne!(light.palette.control, light.palette.panel);
    assert_ne!(light.palette.control, light.palette.content);
    assert_eq!(light.palette.secondary, color(0x77574c));
    assert_eq!(light.palette.on_secondary, color(0xffffff));
    assert_eq!(light.palette.secondary_container, color(0xffdbcf));
    assert_eq!(light.palette.on_secondary_container, color(0x2c160e));
    assert_eq!(light.palette.tertiary, color(0x6a5e2f));
    assert_eq!(light.palette.tertiary_container, color(0xf3e2a7));
    assert_eq!(light.palette.error_container, color(0xffdad6));
    assert_eq!(light.palette.on_error_container, color(0x410002));
    assert_eq!(light.palette.inverse_surface, color(0x392e2b));
    assert_eq!(light.palette.inverse_on_surface, color(0xffede8));
    assert_eq!(light.palette.inverse_primary, color(0xffb59c));
    assert_eq!(light.palette.scrim, color(0x010101));
    assert_eq!(light.palette.source_color, color(0xecccc2));

    let light_tokens = light.palette.tokens_theme(
        Mode::Light,
        &typo,
        motion,
        false,
        crate::appearance::theme::Contrast::Normal,
    );
    assert_eq!(light_tokens.color.secondary, "#ffdbcf");
    assert_eq!(light_tokens.color.secondary_foreground, "#2c160e");

    let dark = dms::parse(source, Path::new("dms-colors.json"), ModePreference::Dark)
        .expect("full DMS dark scheme");
    assert_eq!(dark.palette.surface_container_lowest, color(0x140c0a));
    assert_eq!(dark.palette.surface_container_highest, color(0x322824));
    assert_eq!(dark.palette.panel, color(0x271d1a));
    // Dark neutral control stands above panel (#271d1a) and content (#1a110f) using highest (#322824).
    assert_eq!(dark.palette.control, color(0x322824));
    assert_ne!(dark.palette.control, dark.palette.panel);
    assert_ne!(dark.palette.control, dark.palette.content);
    assert_eq!(dark.palette.secondary_container, color(0x5d4036));
    assert_eq!(dark.palette.on_secondary_container, color(0xffdbcf));
    assert_eq!(dark.palette.tertiary_container, color(0x51461a));
    assert_eq!(dark.palette.error_container, color(0x93000a));
    assert_eq!(dark.palette.scrim, color(0x020202));
}
