use super as dms;
use crate::appearance::theme::{Error, Mode, ModePreference};
use gpui_kit::{Hsla, rgb};
use std::path::Path;

fn color(value: u32) -> Hsla {
    rgb(value).into()
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
    let mut native = gpui_kit::component::ThemeColor::default();
    parsed.palette.apply_to(&mut native);
    assert_eq!(native.border, color(0x444444));
    assert_eq!(native.input, color(0x888888));
    assert_eq!(native.sidebar_accent, color(0x345678));
    assert_eq!(native.sidebar_accent_foreground, color(0xfedcba));
    assert_eq!(native.table_active, native.sidebar_accent);
    assert_eq!(native.accent_foreground, native.sidebar_accent_foreground);
    assert_eq!(native.popover, parsed.palette.panel);
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
        r##"{"mode":"dark","colors":{"dark":{"surface":"not-a-color","on_surface":"#eeeeee","primary":"#abcdef","on_primary":"#123456"}}}"##,
    ] {
        assert!(dms::parse(source, Path::new("dms-colors.json"), ModePreference::Dark).is_err());
    }
}
