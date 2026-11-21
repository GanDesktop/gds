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
    assert_eq!(chosen.family.as_str(), "Noto Sans");
    assert_eq!(chosen.unavailable, None);

    let missing =
        ResolvedFamily::resolve(Some("Missing Sans"), &installed, || "Sarasa UI SC".into());
    assert_eq!(missing.family.as_str(), "Sarasa UI SC");
    assert_eq!(missing.unavailable.as_deref(), Some("Missing Sans"));

    let system = ResolvedFamily::resolve(None, &installed, || "Sarasa UI SC".into());
    assert_eq!(system.family.as_str(), "Sarasa UI SC");
    assert_eq!(system.unavailable, None);

    // An empty list means the platform font system could not be queried, so the
    // configured name is trusted instead of being reported as missing.
    let unverified = ResolvedFamily::resolve(Some("Custom Sans"), &[], || {
        panic!("an unverifiable explicit font must retain its configured family")
    });
    assert_eq!(unverified.family.as_str(), "Custom Sans");
    assert_eq!(unverified.unavailable, None);
}

#[cfg(target_os = "linux")]
mod linux_cache_tests {
    use super::super::linux;
    use std::{cell::Cell, fs, os::unix::fs::symlink};

    fn test_env(lang: &str) -> Vec<(String, Option<String>)> {
        vec![("LANG".to_owned(), Some(lang.to_owned()))]
    }

    #[test]
    fn fingerprint_cache_hits_and_invalidates_on_env_file_symlink_and_corruption() {
        let temp = tempfile::tempdir().expect("temp dir");
        let cache_path = temp.path().join("gds/typography.json");
        let watched_file = temp.path().join("user-fonts.conf");
        let target_a = temp.path().join("a.conf");
        let target_b = temp.path().join("b.conf");
        let watched_link = temp.path().join("00-link.conf");
        let missing_dir = temp.path().join("created-later");

        fs::write(&watched_file, b"<fontconfig/>").expect("write watched file");
        fs::write(&target_a, b"a").expect("write target a");
        fs::write(&target_b, b"b").expect("write target b");
        symlink(&target_a, &watched_link).expect("create symlink");

        let queries = Cell::new(0usize);
        let watched = vec![
            watched_file.clone(),
            watched_link.clone(),
            missing_dir.clone(),
        ];

        // 1. Cold cache triggers query and writes cache.
        let first = linux::load_or_query_with(
            Some(&cache_path),
            || test_env("zh_CN.UTF-8"),
            || {
                queries.set(queries.get() + 1);
                linux::QueriedFamilies {
                    sans_serif: "Sarasa UI SC".to_owned(),
                    monospace: "FiraCode Nerd Font".to_owned(),
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 1);
        assert_eq!(first.sans_serif.as_str(), "Sarasa UI SC");
        assert_eq!(first.monospace.as_str(), "FiraCode Nerd Font");

        // 2. Identical env and watched state hits cache without calling query().
        let second = linux::load_or_query_with(
            Some(&cache_path),
            || test_env("zh_CN.UTF-8"),
            || {
                queries.set(queries.get() + 1);
                linux::QueriedFamilies {
                    sans_serif: "Unexpected".to_owned(),
                    monospace: "Unexpected".to_owned(),
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 1);
        assert_eq!(second.sans_serif.as_str(), "Sarasa UI SC");

        // 3. Env change invalidates cache.
        let env_changed = linux::load_or_query_with(
            Some(&cache_path),
            || test_env("en_US.UTF-8"),
            || {
                queries.set(queries.get() + 1);
                linux::QueriedFamilies {
                    sans_serif: "Noto Sans".to_owned(),
                    monospace: "DejaVu Sans Mono".to_owned(),
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 2);
        assert_eq!(env_changed.sans_serif.as_str(), "Noto Sans");

        // 4. Editing a watched regular file invalidates cache.
        fs::write(&watched_file, b"<fontconfig><!-- updated --></fontconfig>")
            .expect("update watched file");
        let file_changed = linux::load_or_query_with(
            Some(&cache_path),
            || test_env("en_US.UTF-8"),
            || {
                queries.set(queries.get() + 1);
                linux::QueriedFamilies {
                    sans_serif: "Updated Sans".to_owned(),
                    monospace: "Updated Mono".to_owned(),
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 3);
        assert_eq!(file_changed.sans_serif.as_str(), "Updated Sans");

        // 5. Retargeting a watched symlink invalidates cache.
        fs::remove_file(&watched_link).expect("unlink old symlink");
        symlink(&target_b, &watched_link).expect("retarget symlink");
        let link_changed = linux::load_or_query_with(
            Some(&cache_path),
            || test_env("en_US.UTF-8"),
            || {
                queries.set(queries.get() + 1);
                linux::QueriedFamilies {
                    sans_serif: "Symlink Sans".to_owned(),
                    monospace: "Symlink Mono".to_owned(),
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 4);
        assert_eq!(link_changed.sans_serif.as_str(), "Symlink Sans");

        // 6. Creating a previously Missing watched directory invalidates cache.
        fs::create_dir(&missing_dir).expect("create previously missing watched dir");
        let created = linux::load_or_query_with(
            Some(&cache_path),
            || test_env("en_US.UTF-8"),
            || {
                queries.set(queries.get() + 1);
                linux::QueriedFamilies {
                    sans_serif: "Created Sans".to_owned(),
                    monospace: "Created Mono".to_owned(),
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 5);
        assert_eq!(created.sans_serif.as_str(), "Created Sans");

        // 7. Corrupt JSON recovers by re-querying and overwriting cache.
        fs::write(&cache_path, b"{corrupt json").expect("corrupt cache");
        let recovered = linux::load_or_query_with(
            Some(&cache_path),
            || test_env("en_US.UTF-8"),
            || {
                queries.set(queries.get() + 1);
                linux::QueriedFamilies {
                    sans_serif: "Recovered Sans".to_owned(),
                    monospace: "Recovered Mono".to_owned(),
                    watched: watched.clone(),
                }
            },
        );
        assert_eq!(queries.get(), 6);
        assert_eq!(recovered.sans_serif.as_str(), "Recovered Sans");
    }

    #[test]
    fn live_fontconfig_resolution_returns_usable_defaults_and_rejects_unknown_custom_font() {
        assert!(!super::super::system_family().is_empty());
        assert!(!super::super::system_mono_family().is_empty());
        assert!(linux::resolve_custom_monospace("__GDS_Nonexistent_Font__").is_none());
    }
}
