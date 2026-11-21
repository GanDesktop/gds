use super as kde;
use crate::appearance::theme::{Mode, ModePreference, provider::color::rgb};
use std::path::Path;
use zgui::canvas::zgui_color::Color;

fn color(value: u32) -> Color {
    rgb(value)
}

#[test]
fn plasma_uses_active_colors_and_keeps_kconfig_subgroups_separate() {
    let source = "[General]\nColorScheme=BreezeLight\n\
                  [Colors:Window]\nBackgroundNormal=30,30,30\nForegroundNormal=240,240,240\n\
                  [Colors:Window][Inactive]\nBackgroundNormal=250,250,250\n\
                  [Colors:View]\nBackgroundNormal=20,20,20\nForegroundNormal=230,230,230\n\
                  [Colors:Selection]\nBackgroundNormal=120,30,90\nForegroundNormal=255,240,250\n";
    let path = Path::new("kdeglobals");
    let loaded = kde::parse(source, path, ModePreference::System)
        .expect("active KDE palette, including nested KConfig groups");
    assert_eq!(loaded.mode, Mode::Dark);
    assert_eq!(loaded.palette.sidebar, color(0x1e1e1e));
    assert_eq!(loaded.palette.sidebar_foreground, color(0xf0f0f0));
    assert_eq!(loaded.palette.content, color(0x141414));
    assert_eq!(loaded.palette.foreground, color(0xe6e6e6));
    assert_eq!(loaded.palette.primary, color(0x781e5a));
    assert_eq!(loaded.palette.primary_foreground, color(0xfff0fa));
    assert_eq!(loaded.source_path.as_deref(), Some(path));

    let light = source.replace("BackgroundNormal=30,30,30", "BackgroundNormal=240,240,240");
    let loaded =
        kde::parse(&light, path, ModePreference::System).expect("the newly applied light palette");
    assert_eq!(loaded.mode, Mode::Light);

    let explicit = kde::parse(source, path, ModePreference::Light)
        .expect("explicit mode keeps the one active Plasma palette");
    assert_eq!(explicit.mode, Mode::Light);
    assert_eq!(explicit.palette.sidebar, color(0x1e1e1e));

    assert!(
        kde::parse(
            "[General]\nColorScheme=BreezeLight\n",
            path,
            ModePreference::System
        )
        .is_err()
    );
}

#[test]
fn kde_groups_support_rgb_triplets_hex_colors_and_reject_invalid_channels() {
    let source = "[Colors:Window]\nBackgroundNormal=10,20,30,255\nForegroundNormal=#d2dce6\n\
                  [Colors:View]\nBackgroundNormal=1,2,3\nForegroundNormal=#ffc9cacb\n\
                  [Colors:Selection]\nBackgroundNormal=40,50,60\nForegroundNormal=240,250,255\n";
    let path = Path::new("kdeglobals");
    let loaded = kde::parse(source, path, ModePreference::System).expect("valid KDE color groups");
    assert_eq!(loaded.palette.sidebar, color(0x0a141e));
    assert_eq!(loaded.palette.sidebar_foreground, color(0xd2dce6));
    assert_eq!(loaded.palette.content, color(0x010203));
    assert_eq!(loaded.palette.foreground, color(0xc9cacb));
    assert_eq!(loaded.palette.selection_foreground, color(0xf0faff));
    assert_eq!(loaded.palette.surface, loaded.palette.sidebar);
    assert_eq!(loaded.palette.tooltip, loaded.palette.sidebar);
    assert_eq!(loaded.palette.panel, loaded.palette.surface);
    assert_eq!(
        loaded.palette.panel_foreground,
        loaded.palette.surface_foreground
    );

    assert!(
        kde::parse(
            &source.replace("1,2,3", "999,2,3"),
            path,
            ModePreference::Dark
        )
        .is_err()
    );
}
