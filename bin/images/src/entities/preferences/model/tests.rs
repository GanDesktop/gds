use super::{DEFAULT_JPEG_QUALITY, Preferences};
use gds::appearance::{theme, typography};

#[test]
fn preferences_toml_roundtrip_preserves_fields_and_ignores_legacy_keys() {
    let custom = Preferences {
        appearance: theme::Appearance {
            source: theme::Source::Builtin,
            builtin: theme::BuiltinTheme::Neon,
            mode: theme::ModePreference::Dark,
            content_opacity: 0.9,
            sidebar_opacity: 0.75,
            rounded_corners: true,
        },
        animations_enabled: Some(false),
        font: typography::Preferences {
            family: None,
            size: 15.0,
        },
        checkerboard_backdrop: false,
        strip_privacy_on_copy: true,
        default_jpeg_quality: 85,
        inspector_open: false,
        filmstrip_open: false,
    };
    let encoded = toml::to_string(&custom).expect("serialize preferences to TOML");
    let decoded: Preferences = toml::from_str(&encoded).expect("parse TOML back");
    assert_eq!(decoded, custom);

    // Older configs containing removed `smooth_scaling` or renamed keys must still deserialize.
    let partial: Preferences =
        toml::from_str("smooth_scaling = false\ninspector_panel_width = 248\n")
            .expect("partial TOML");
    assert!(partial.checkerboard_backdrop);
    assert!(partial.filmstrip_open);
    assert_eq!(partial.default_jpeg_quality, DEFAULT_JPEG_QUALITY);
    assert_eq!(partial.appearance, theme::Appearance::default());
    assert_eq!(partial.animations_enabled, None);
    assert_eq!(partial.font, typography::Preferences::default());
}
