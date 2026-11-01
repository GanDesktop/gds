use super::*;
use crate::appearance::theme::{Appearance, Source};

#[test]
fn embedded_catalog_loads_both_modes_without_runtime_files() {
    for theme in Theme::ALL {
        let mut variants = Vec::new();
        for mode in [ModePreference::Light, ModePreference::Dark] {
            let appearance = Appearance {
                source: Source::Builtin,
                builtin: theme,
                mode,
                ..Default::default()
            };
            let prepared = appearance
                .prepare(WindowAppearance::Dark)
                .expect("complete embedded palette");
            assert!(prepared.source_paths().is_empty());
            assert_eq!(prepared.loaded.mode, mode.resolve(WindowAppearance::Dark));
            variants.push(prepared.loaded.palette);
        }
        assert_ne!(variants[0], variants[1]);
    }
    for mode in [Mode::Light, Mode::Dark] {
        assert_ne!(Theme::Bamboo.palette(mode), Theme::Retro.palette(mode));
        assert_ne!(Theme::Retro.palette(mode), Theme::Neon.palette(mode));
        assert_ne!(Theme::Bamboo.palette(mode), Theme::Neon.palette(mode));
    }
}

#[test]
fn embedded_choice_round_trips_and_keeps_system_mode_independent() {
    for theme in Theme::ALL {
        let appearance = Appearance {
            source: Source::Builtin,
            builtin: theme,
            ..Default::default()
        };
        let saved = toml::to_string(&appearance).expect("serialize theme choice");
        let restored: Appearance = toml::from_str(&saved).expect("restore theme choice");
        assert_eq!(restored, appearance);
        for platform in [WindowAppearance::Light, WindowAppearance::Dark] {
            let prepared = restored.prepare(platform).expect("resolve system mode");
            assert_eq!(
                prepared.loaded.mode,
                ModePreference::System.resolve(platform)
            );
            assert_eq!(prepared.loaded.palette, theme.palette(prepared.loaded.mode));
        }
    }
}
