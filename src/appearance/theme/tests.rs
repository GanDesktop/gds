use super::{Appearance, Error, Mode, ModePreference, Source, validate_opacity};
use crate::app::Gds;
use crate::service::task_manager::Scope;
use zgui::reactive::{Mounted, install};

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
    for removed in ["qt5ct", "qt6ct"] {
        assert!(toml::from_str::<Appearance>(&format!("source = '{removed}'")).is_err());
    }
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
    assert!(requested.prepare(Mode::Dark).is_err());
    let fallback = requested.builtin_fallback();
    let prepared = fallback.prepare(Mode::Dark).expect("local fallback");
    assert_eq!(prepared.appearance.content_opacity, 1.0);
    assert_eq!(prepared.appearance.sidebar_opacity, 0.8);
    assert_eq!(prepared.appearance.builtin, super::BuiltinTheme::Neon);
    assert_eq!(prepared.loaded.mode, Mode::Light);
    assert!(prepared.appearance.rounded_corners);
}

#[test]
fn platform_appearance_is_only_used_for_system_mode() {
    assert_eq!(ModePreference::System.resolve(Mode::Light), Mode::Light);
    assert_eq!(ModePreference::System.resolve(Mode::Dark), Mode::Dark);
    assert_eq!(ModePreference::Light.resolve(Mode::Dark), Mode::Light);
    assert_eq!(ModePreference::Dark.resolve(Mode::Light), Mode::Dark);
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

#[test]
fn reactive_appearance_emits_gds_and_zui_css_tokens() {
    install().expect("install reactive runtime");
    let root = Mounted::new();
    root.with(|| {
        let gds = Gds::install(Scope::default());
        let app = gds.appearance();
        let initial_css = app.full_stylesheet_css();
        assert!(initial_css.contains("--zui-color-primary:"));
        assert!(initial_css.contains("--gds-content-bg:"));
        assert!(initial_css.contains("--gds-sidebar-bg:"));
        assert!(initial_css.contains("--gds-foreground:"));
        assert!(initial_css.contains("--gds-radius-window: 0px;"));
        assert!(initial_css.contains("--gds-radius-control: 6px;"));
        assert!(initial_css.contains("--gds-term-ansi-0:"));

        let initial_tokens = app.tokens_theme();
        assert_eq!(initial_tokens.radius.sm, "4px");
        assert_eq!(initial_tokens.radius.md, "6px");
        assert_eq!(initial_tokens.radius.lg, "8px");
        assert_eq!(initial_tokens.radius.xl, "10px");
        assert_eq!(initial_tokens.radius.base, "8px");
        assert_eq!(initial_tokens.focus.ring_shadow, "none");
        assert_eq!(initial_tokens.focus.ring_invalid_shadow, "none");
        assert_eq!(initial_tokens.focus.sidebar_ring_shadow, "none");
        assert_eq!(initial_tokens.focus.ring_inset_shadow, "none");
        assert_ne!(
            initial_tokens.control.ghost_hover, initial_tokens.color.accent,
            "ghost hover must use neutral 8% foreground tint instead of selection accent"
        );
        assert_ne!(
            initial_tokens.control.outline_hover, initial_tokens.color.accent,
            "outline hover must use opaque neutral control hover instead of selection accent"
        );
        assert_ne!(
            initial_tokens.control.ghost_hover, initial_tokens.control.outline_hover,
            "raised outline hover must stay opaque over --gds-control while ghost hover stays translucent"
        );

        app.theme()
            .apply(&Appearance {
                source: Source::Builtin,
                builtin: super::BuiltinTheme::Neon,
                mode: ModePreference::Light,
                content_opacity: 0.75,
                sidebar_opacity: 0.5,
                rounded_corners: true,
            })
            .expect("apply builtin neon light");
        assert_eq!(app.theme().mode(), Mode::Light);
        assert!(app.theme().rounded_corners());
        assert!(app.theme().material().is_translucent());
        assert!((app.theme().content_background().alpha() - 0.75).abs() < 1e-4);

        let updated_css = app.full_stylesheet_css();
        assert!(updated_css.contains("color-scheme: light;"));
        assert!(updated_css.contains("--gds-radius-window: 10px;"));
        assert!(updated_css.contains("--gds-content-opacity: 0.75;"));
        assert!(updated_css.contains("--gds-sidebar-opacity: 0.5;"));
        assert_eq!(app.tokens_theme().radius.base, "8px");
        assert_eq!(app.tokens_theme().radius.md, "6px");

        app.typography()
            .apply(&crate::appearance::typography::Preferences {
                family: None,
                size: 20.0,
            })
            .expect("apply font scale");
        assert_eq!(app.typography().size(), 20.0);
        let scaled_css = app.gds_stylesheet_css();
        assert!(scaled_css.contains("--gds-font-size: 20.00px;"));
        assert!(scaled_css.contains("--gds-font-size-2xl: 30.00px;"));
    });
    root.unmount();
}
