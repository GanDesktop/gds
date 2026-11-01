use super as qt;
use crate::appearance::theme::{Error, Mode, ModePreference};
use gpui_kit::{Hsla, rgb};
use std::path::Path;

fn color(value: u32) -> Hsla {
    rgb(value).into()
}

#[test]
fn qt_configuration_controls_the_source_path() {
    let config = Path::new("/config/qt6ct/qt6ct.conf");
    let home = Path::new("/home/test");
    let relative =
        "[Appearance]\ncustom_palette=true\ncolor_scheme_path=\"colors/My theme.conf\"\n";
    assert_eq!(
        qt::palette_path(relative, config, home).expect("relative palette path"),
        Path::new("/config/qt6ct/colors/My theme.conf")
    );
    let tilde = "[Appearance]\ncustom_palette=true\ncolor_scheme_path=~/themes/custom.colors\n";
    assert_eq!(
        qt::palette_path(tilde, config, home).expect("home palette path"),
        Path::new("/home/test/themes/custom.colors")
    );
    let disabled = "[Appearance]\ncustom_palette=false\ncolor_scheme_path=/unused.conf\n";
    assert!(matches!(
        qt::palette_path(disabled, config, home),
        Err(Error::CustomPaletteDisabled(_))
    ));
    assert!(qt::palette_path("[Appearance]\ncustom_palette=true\n", config, home).is_err());
}

#[test]
fn qt_role_arrays_map_paired_colors_and_preserve_disabled_text() {
    let active = (0..22)
        .map(|index| format!("#ff{:06x}", index * 0x010101))
        .collect::<Vec<_>>()
        .join(", ");
    let disabled = (0..22)
        .map(|index| format!("#ff{:06x}", (index + 40) * 0x010101))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "[ColorScheme]\nactive_colors={active}\ninactive_colors={active}\ndisabled_colors={disabled}\n"
    );
    let palette =
        qt::parse(&source, Path::new("palette.conf"), Mode::Dark).expect("valid Qt role arrays");
    assert_eq!(palette.content, color(9 * 0x010101));
    assert_eq!(palette.foreground, color(6 * 0x010101));
    assert_eq!(palette.sidebar, color(10 * 0x010101));
    assert_eq!(palette.selection, color(12 * 0x010101));
    assert_eq!(palette.selection_foreground, color(13 * 0x010101));
    assert_eq!(palette.muted_foreground, color(46 * 0x010101));
    assert_eq!(palette.panel, palette.surface);
    assert_eq!(palette.panel_foreground, palette.surface_foreground);
}

#[test]
fn qt_rejects_truncated_and_invalid_state_arrays() {
    assert!(
        qt::parse(
            "[ColorScheme]\nactive_colors=#ffffff\n",
            Path::new("palette.conf"),
            Mode::Light
        )
        .is_err()
    );
    let active = vec!["#ffffff"; 21].join(",");
    let source = format!("[ColorScheme]\nactive_colors={active}\ninactive_colors=invalid\n");
    assert!(qt::parse(&source, Path::new("palette.conf"), Mode::Light).is_err());
}

#[test]
fn kde_groups_are_detected_by_content_not_file_extension() {
    let source = "[Colors:Window]\nBackgroundNormal=10,20,30\nForegroundNormal=210,220,230\n\
                  [Colors:View]\nBackgroundNormal=1,2,3\nForegroundNormal=201,202,203\n\
                  [Colors:Selection]\nBackgroundNormal=40,50,60\nForegroundNormal=240,250,255\n";
    let palette = qt::parse(source, Path::new("not-named-colors.conf"), Mode::Dark)
        .expect("valid KDE basic groups");
    assert_eq!(palette.content, color(0x010203));
    assert_eq!(palette.sidebar, color(0x0a141e));
    assert_eq!(palette.selection_foreground, color(0xf0faff));
    assert!(
        qt::parse(
            &source.replace("1,2,3", "999,2,3"),
            Path::new("invalid.colors"),
            Mode::Dark
        )
        .is_err()
    );
}

#[test]
fn plasma_uses_active_colors_and_keeps_kconfig_subgroups_separate() {
    let source = "[General]\nColorScheme=BreezeLight\n\
                  [Colors:Window]\nBackgroundNormal=30,30,30\nForegroundNormal=240,240,240\n\
                  [Colors:Window][Inactive]\nBackgroundNormal=250,250,250\n\
                  [Colors:View]\nBackgroundNormal=20,20,20\nForegroundNormal=230,230,230\n\
                  [Colors:Selection]\nBackgroundNormal=120,30,90\nForegroundNormal=255,240,250\n";
    let path = Path::new("kdeglobals");
    let loaded = qt::parse_kde_system(source, path, ModePreference::System)
        .expect("active KDE palette, including nested KConfig groups");
    assert_eq!(loaded.mode, Mode::Dark);
    assert_eq!(loaded.palette.sidebar, color(0x1e1e1e));
    assert_eq!(loaded.palette.primary, color(0x781e5a));
    assert_eq!(loaded.palette.primary_foreground, color(0xfff0fa));
    assert_eq!(loaded.source_path.as_deref(), Some(path));

    let light = source.replace("BackgroundNormal=30,30,30", "BackgroundNormal=240,240,240");
    let loaded = qt::parse_kde_system(&light, path, ModePreference::System)
        .expect("the newly applied light palette");
    assert_eq!(loaded.mode, Mode::Light);

    let explicit = qt::parse_kde_system(source, path, ModePreference::Light)
        .expect("explicit mode keeps the one active Plasma palette");
    assert_eq!(explicit.mode, Mode::Light);
    assert_eq!(explicit.palette.sidebar, color(0x1e1e1e));
    assert!(
        qt::parse_kde_system(
            "[General]\nColorScheme=BreezeLight\n",
            path,
            ModePreference::System
        )
        .is_err()
    );
}
