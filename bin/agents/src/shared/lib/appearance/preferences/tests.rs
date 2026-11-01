use super::*;

#[test]
fn omitted_appearance_fields_use_shared_defaults_and_the_desktop_theme() {
    let preferences = Preferences::from_json("{}").expect("empty legacy appearance");
    let effective = resolve(&preferences).expect("shared defaults");
    let appearance = appearance(&preferences);
    assert_eq!(effective.font, typography::Preferences::default());
    assert_eq!(
        appearance,
        theme::Appearance {
            source: theme::Source::System,
            ..Default::default()
        }
    );
    assert_eq!(
        effective.background,
        gds::appearance::tokens::WINDOW_BACKGROUND
    );
    assert_eq!(
        effective.content_font_size,
        typography::DEFAULT_SIZE * gds::appearance::tokens::UI_TEXT_SCALE
    );
}

#[test]
fn explicit_appearance_overrides_survive_the_shared_defaults() {
    let preferences = Preferences::from_json(
        r#"{"ui_font_size":20,"content_font_size":18,"content_opacity":0.7,"sidebar_opacity":0.3,"background":"blurred"}"#,
    )
    .expect("existing appearance");
    let effective = resolve(&preferences).expect("explicit preferences");
    assert_eq!(effective.font.size, 20.);
    assert_eq!(effective.content_font_size, 18.);
    assert_eq!(effective.background, WindowBackgroundAppearance::Blurred);
    assert_eq!(appearance(&preferences).content_opacity, 0.7);
    assert_eq!(appearance(&preferences).sidebar_opacity, 0.3);
    assert_eq!(effective.preferences, preferences);
}

#[test]
fn sizes_and_opacity_are_validated_for_json_and_programmatic_callers() {
    for name in ["ui_font_size", "content_font_size", "content_line_height"] {
        for value in ["0", "-1", "1e100", "\"large\""] {
            assert!(Preferences::from_json(&format!(r#"{{"{name}":{value}}}"#)).is_err());
        }
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0., -1.] {
        assert!(
            Preferences {
                ui_font_size: Some(value),
                ..Preferences::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Preferences {
                content_font_size: Some(value),
                ..Preferences::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Preferences {
                content_line_height: Some(value),
                ..Preferences::default()
            }
            .validate()
            .is_err()
        );
    }
    for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        for preferences in [
            Preferences {
                content_opacity: Some(value),
                ..Preferences::default()
            },
            Preferences {
                sidebar_opacity: Some(value),
                ..Preferences::default()
            },
        ] {
            assert!(preferences.validate().is_err());
        }
    }
    for value in [0., 1.] {
        assert!(
            Preferences {
                content_opacity: Some(value),
                sidebar_opacity: Some(value),
                ..Preferences::default()
            }
            .validate()
            .is_ok()
        );
    }
    assert!(Preferences::from_json(r#"{"ui_font_sze":17}"#).is_err());
    assert!(Preferences::from_json(r#"{"background":"glass"}"#).is_err());
}
