use super::{DEFAULT_SIZE, MAX_SIZE, MIN_SIZE, Preferences, ResolvedFamily, validate_size};

#[test]
fn legacy_configuration_keeps_the_system_font_and_the_current_size() {
    let legacy: Preferences = toml::from_str("").expect("empty configuration");
    assert_eq!(legacy, Preferences::default());
    assert_eq!(legacy.family, None);
    assert_eq!(legacy.size, DEFAULT_SIZE);

    let family_only: Preferences =
        toml::from_str("family = \"Noto Sans\"").expect("family without a size");
    assert_eq!(family_only.family.as_deref(), Some("Noto Sans"));
    assert_eq!(family_only.size, DEFAULT_SIZE);

    let configured: Preferences =
        toml::from_str("family = \"Noto Sans\"\nsize = 18.0\n").expect("configured typography");
    assert_eq!(configured.family.as_deref(), Some("Noto Sans"));
    assert_eq!(configured.size, 18.0);

    let encoded = toml::to_string(&configured).expect("serializable typography");
    assert_eq!(
        toml::from_str::<Preferences>(&encoded).expect("reload typography"),
        configured
    );
}

#[test]
fn the_default_size_scales_explicit_text_by_one() {
    assert_eq!(Preferences::default().scale(), 1.0);
    let larger = Preferences {
        size: MAX_SIZE,
        ..Preferences::default()
    };
    assert_eq!(larger.scale(), MAX_SIZE / DEFAULT_SIZE);
}

#[test]
fn non_finite_and_out_of_range_sizes_are_rejected() {
    for value in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -1.0,
        0.0,
        MIN_SIZE - 1.0,
        MAX_SIZE + 1.0,
    ] {
        assert!(validate_size(value).is_err(), "{value} must be rejected");
    }
    for value in [MIN_SIZE, DEFAULT_SIZE, MAX_SIZE] {
        assert!(validate_size(value).is_ok(), "{value} must be accepted");
    }
}

#[test]
fn a_configured_nan_size_parses_but_must_not_be_applied() {
    let preferences: Preferences = toml::from_str("size = nan").expect("TOML admits NaN floats");
    assert!(validate_size(preferences.size).is_err());
}

#[test]
fn only_installed_or_unverifiable_families_are_used_directly() {
    let installed = vec!["Noto Sans".to_owned()];

    let chosen = ResolvedFamily::resolve(Some("Noto Sans"), &installed, || {
        panic!("an installed explicit font must not query the system default")
    });
    assert_eq!(chosen.family.as_ref(), "Noto Sans");
    assert_eq!(chosen.unavailable, None);

    let missing =
        ResolvedFamily::resolve(Some("Missing Sans"), &installed, || "Sarasa UI SC".into());
    assert_eq!(missing.family.as_ref(), "Sarasa UI SC");
    assert_eq!(missing.unavailable.as_deref(), Some("Missing Sans"));

    let system = ResolvedFamily::resolve(None, &installed, || "Sarasa UI SC".into());
    assert_eq!(system.family.as_ref(), "Sarasa UI SC");
    assert_eq!(system.unavailable, None);

    // An empty list means the platform font system could not be queried, so the
    // configured name is trusted instead of being reported as missing.
    let unverified = ResolvedFamily::resolve(Some("Custom Sans"), &[], || {
        panic!("an unverifiable explicit font must retain its configured family")
    });
    assert_eq!(unverified.family.as_ref(), "Custom Sans");
    assert_eq!(unverified.unavailable, None);
}
