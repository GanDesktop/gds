use gpui_kit::WindowAppearance;

#[cfg(feature = "gui-tests")]
#[gpui_kit::test]
fn palette_reinstallation_preserves_thin_semantic_focus(cx: &mut gpui_kit::TestAppContext) {
    use crate::app::AppExt as _;
    use gpui_kit::{
        Styled,
        component::{Theme, ThemeStyled},
        div, px, size,
    };

    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(300.), px(80.)), |_, _| gpui_kit::Empty);
    for builtin in super::BuiltinTheme::ALL {
        for mode in [ModePreference::Light, ModePreference::Dark] {
            let appearance = Appearance {
                source: Source::Builtin,
                builtin,
                mode,
                ..Appearance::default()
            };
            // Initial/manual apply and the prepared hot-reload path must agree,
            // even when Kit's previous theme enabled its external ring.
            for prepared in [false, true] {
                window
                    .update(cx, |_, window, cx| {
                        Theme::update(cx, |theme| theme.focus_ring = true);
                        if prepared {
                            let prepared = appearance
                                .prepare(WindowAppearance::Dark)
                                .expect("prepare palette");
                            cx.gds()
                                .appearance()
                                .theme()
                                .install(prepared)
                                .expect("reload palette");
                        } else {
                            cx.gds()
                                .appearance()
                                .theme()
                                .apply(&appearance)
                                .expect("apply palette");
                        }
                        let theme = Theme::global(cx);
                        assert!(
                            !theme.focus_ring,
                            "{builtin:?} {mode:?} must not restore outer halos"
                        );
                        assert_eq!(
                            theme.ring,
                            cx.gds_ro().appearance().theme().colors().primary
                        );
                        assert!(theme.ring.a > 0.0, "focus ink must remain visible");
                        let mut control = div()
                            .w(px(100.))
                            .h(px(32.))
                            .border_1()
                            .rounded(theme.radius);
                        let before = control.style().clone();
                        let mut focused = control.focus_ring_style(window, cx);
                        let after = focused.style();
                        assert_eq!(before.size, after.size);
                        assert_eq!(before.border_widths, after.border_widths);
                        assert_eq!(before.corner_radii, after.corner_radii);
                        assert_eq!(after.border_color, Some(theme.ring));
                    })
                    .expect("check installed focus policy");
            }
        }
    }
}

use super::{Appearance, Error, Mode, ModePreference, Source, validate_opacity};

#[test]
fn appearance_defaults_follow_system_without_changing_explicit_legacy_choices() {
    let appearance: Appearance = serde_json::from_str("{}").expect("valid settings JSON");
    assert_eq!(appearance, Appearance::default());
    assert_eq!(appearance.source, Source::System);
    assert_eq!(appearance.mode, ModePreference::System);
    assert!(!appearance.rounded_corners);
    let explicit: Appearance = toml::from_str("source = 'dms'").expect("explicit DMS selection");
    assert_eq!(explicit.source, Source::System);
    let encoded = toml::to_string(&explicit).expect("system appearance serialization");
    assert!(encoded.contains("source = \"system\""));
    assert_eq!(
        toml::from_str::<Appearance>(&encoded).expect("system appearance"),
        explicit
    );
    for (stored, expected) in [
        ("light", ModePreference::Light),
        ("dark", ModePreference::Dark),
    ] {
        let source = format!("source = 'builtin'\nmode = '{stored}'\n");
        let legacy: Appearance = toml::from_str(&source).expect("legacy appearance configuration");
        assert_eq!(legacy.source, Source::Builtin);
        assert_eq!(legacy.builtin, super::BuiltinTheme::Bamboo);
        assert_eq!(legacy.mode, expected);
        let encoded = serde_json::to_value(legacy).expect("serializable appearance");
        assert_eq!(encoded["mode"], stored);
    }
}

#[test]
fn builtin_fallback_keeps_the_mode_intent_and_uses_safe_material_defaults() {
    for mode in [
        ModePreference::System,
        ModePreference::Light,
        ModePreference::Dark,
    ] {
        let requested = Appearance {
            source: Source::System,
            builtin: Default::default(),
            mode,
            content_opacity: f32::NAN,
            sidebar_opacity: -1.0,
            rounded_corners: true,
        };
        let fallback = requested.builtin_fallback();
        assert_eq!(fallback.source, Source::Builtin);
        assert_eq!(fallback.mode, mode);
        assert!(fallback.rounded_corners);
        assert_eq!(
            fallback.content_opacity,
            Appearance::default().content_opacity
        );
        assert_eq!(
            fallback.sidebar_opacity,
            Appearance::default().sidebar_opacity
        );
        assert_eq!(requested.source, Source::System);
        assert!(requested.content_opacity.is_nan());
    }
}

#[test]
fn missing_palette_fallback_preserves_valid_opacity_and_selected_builtin() {
    let directory = tempfile::tempdir().expect("isolated theme directory");
    let requested = Appearance {
        source: Source::Custom(directory.path().join("missing.toml")),
        builtin: super::BuiltinTheme::Neon,
        mode: ModePreference::Light,
        content_opacity: 1.0,
        sidebar_opacity: 0.8,
        rounded_corners: true,
    };
    assert!(requested.prepare(WindowAppearance::Dark).is_err());
    let fallback = requested.builtin_fallback();
    let prepared = fallback
        .prepare(WindowAppearance::Dark)
        .expect("local fallback");
    assert_eq!(prepared.appearance.content_opacity, 1.0);
    assert_eq!(prepared.appearance.sidebar_opacity, 0.8);
    assert_eq!(prepared.appearance.builtin, super::BuiltinTheme::Neon);
    assert_eq!(prepared.loaded.mode, Mode::Light);
    assert!(prepared.appearance.rounded_corners);
}

#[test]
fn platform_appearance_is_only_used_for_system_mode() {
    assert_eq!(
        ModePreference::System.resolve(WindowAppearance::VibrantLight),
        Mode::Light
    );
    assert_eq!(
        ModePreference::System.resolve(WindowAppearance::VibrantDark),
        Mode::Dark
    );
    assert_eq!(
        ModePreference::Light.resolve(WindowAppearance::Dark),
        Mode::Light
    );
    assert_eq!(
        ModePreference::Dark.resolve(WindowAppearance::Light),
        Mode::Dark
    );
}

#[test]
fn invalid_opacity_is_rejected_instead_of_silently_clamped() {
    for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        assert!(matches!(
            validate_opacity("content_opacity", value),
            Err(Error::InvalidOpacity { .. })
        ));
    }
    assert!(validate_opacity("content_opacity", 0.0).is_ok());
    assert!(validate_opacity("content_opacity", 1.0).is_ok());
}
