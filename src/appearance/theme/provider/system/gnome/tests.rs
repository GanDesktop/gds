use super::*;

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

#[test]
fn adwaita_light_and_dark_defaults_resolve_without_python_or_files() {
    let dir = tempfile::tempdir().expect("isolated config directory");
    let portal = PortalSnapshot::default();

    let light = resolve(
        ModePreference::System,
        WindowAppearance::Light,
        portal,
        dir.path(),
    )
    .expect("default Adwaita light palette");
    assert_eq!(light.mode, Mode::Light);
    assert_eq!(light.palette.content, hex(0xffffff));
    assert_eq!(light.palette.sidebar, hex(0xebebeb));
    assert_eq!(light.palette.primary, hex(0x3584e4));
    assert_eq!(light.palette.primary_foreground, hex(0xffffff));
    assert_eq!(light.palette.terminal.selection, light.palette.selection);

    let dark = resolve(
        ModePreference::System,
        WindowAppearance::Dark,
        portal,
        dir.path(),
    )
    .expect("default Adwaita dark palette");
    assert_eq!(dark.mode, Mode::Dark);
    assert_eq!(dark.palette.content, hex(0x1e1e1e));
    assert_eq!(dark.palette.sidebar, hex(0x303030));
    assert_eq!(dark.palette.foreground, hex(0xffffff));
}

#[test]
fn portal_color_scheme_and_accent_color_drive_primary_and_contrast() {
    let dir = tempfile::tempdir().expect("isolated config directory");
    let bright_accent = PortalSnapshot {
        color_scheme: Some(Mode::Dark),
        accent_color: Some(Rgba {
            r: 0.95,
            g: 0.85,
            b: 0.25,
            a: 1.0,
        }),
        primed: true,
    };

    let loaded = resolve(
        ModePreference::System,
        WindowAppearance::Light,
        bright_accent,
        dir.path(),
    )
    .expect("Portal dark + accent palette");
    assert_eq!(loaded.mode, Mode::Dark);
    assert_eq!(
        loaded.palette.primary,
        Hsla::from(bright_accent.accent_color.expect("accent color"))
    );
    assert_eq!(loaded.palette.primary_foreground, hex(0x1e1e1e));

    let explicit_light = resolve(
        ModePreference::Light,
        WindowAppearance::Dark,
        bright_accent,
        dir.path(),
    )
    .expect("explicit Light overrides Portal color-scheme");
    assert_eq!(explicit_light.mode, Mode::Light);
    assert_eq!(explicit_light.palette.content, hex(0xffffff));
}

#[test]
fn settings_ini_dark_preference_applies_when_portal_scheme_is_unset() {
    let dir = tempfile::tempdir().expect("isolated config directory");
    let gtk4 = dir.path().join("gtk-4.0");
    std::fs::create_dir_all(&gtk4).expect("gtk-4.0 dir");
    std::fs::write(
        gtk4.join("settings.ini"),
        "[Settings]\ngtk-application-prefer-dark-theme=true\n",
    )
    .expect("write settings.ini");

    let loaded = resolve(
        ModePreference::System,
        WindowAppearance::Light,
        PortalSnapshot::default(),
        dir.path(),
    )
    .expect("settings.ini dark theme");
    assert_eq!(loaded.mode, Mode::Dark);
    assert_eq!(loaded.palette.content, hex(0x1e1e1e));
}

#[test]
fn gtk3_and_gtk4_css_define_color_overrides_resolve_with_gtk4_precedence() {
    let dir = tempfile::tempdir().expect("isolated config directory");
    let gtk3 = dir.path().join("gtk-3.0");
    let gtk4 = dir.path().join("gtk-4.0");
    std::fs::create_dir_all(&gtk3).expect("gtk-3.0 dir");
    std::fs::create_dir_all(&gtk4).expect("gtk-4.0 dir");

    std::fs::write(
        gtk3.join("gtk.css"),
        r#"
        /* legacy GTK 3 override */
        @define-color theme_base_color #102030;
        @define-color theme_bg_color rgb(16, 24, 32);
        @define-color theme_fg_color #eeeeee;
        @define-color theme_text_color #f5f5f5;
        @define-color accent_bg_color @purple_3;
        @define-color borders rgba(255, 255, 255, 0.25);
        "#,
    )
    .expect("write gtk3 css");
    std::fs::write(
        gtk4.join("gtk.css"),
        r#"
        /* GTK 4 override wins over GTK 3 */
        @define-color custom_brand #112233;
        @define-color accent_bg_color @custom_brand;
        @define-color sidebar_bg_color rgb(12, 18, 24);
        @define-color insensitive_fg_color alpha(@theme_text_color, 0.5);
        "#,
    )
    .expect("write gtk4 css");

    let loaded = resolve(
        ModePreference::System,
        WindowAppearance::Light,
        PortalSnapshot::default(),
        dir.path(),
    )
    .expect("CSS-customized GNOME palette");

    assert_eq!(loaded.mode, Mode::Dark);
    assert_eq!(loaded.palette.content, hex(0x102030));
    assert_eq!(
        loaded.palette.surface,
        Rgba {
            r: 16.0 / 255.0,
            g: 24.0 / 255.0,
            b: 32.0 / 255.0,
            a: 1.0
        }
        .into()
    );
    assert_eq!(
        loaded.palette.sidebar,
        Rgba {
            r: 12.0 / 255.0,
            g: 18.0 / 255.0,
            b: 24.0 / 255.0,
            a: 1.0
        }
        .into()
    );
    assert_eq!(loaded.palette.primary, hex(0x112233));
    assert_eq!(loaded.palette.selection, hex(0x112233));
    assert_eq!(loaded.palette.foreground, hex(0xf5f5f5));
    assert_eq!(loaded.palette.tooltip, loaded.palette.panel);
}
