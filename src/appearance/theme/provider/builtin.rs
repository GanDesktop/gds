//! Embedded TOML palettes, parsed once and independent of runtime files.
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use super::{Loaded, Mode, ModePreference, Palette, schema::Definition};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Bamboo,
    Retro,
    Neon,
    Macaron,
    HighContrast,
    Aero,
}

impl Theme {
    pub const ALL: [Self; 6] = [
        Self::Bamboo,
        Self::Retro,
        Self::Neon,
        Self::Macaron,
        Self::HighContrast,
        Self::Aero,
    ];

    pub fn label_id(self) -> &'static str {
        match self {
            Self::Bamboo => "theme-bamboo",
            Self::Retro => "theme-retro",
            Self::Neon => "theme-neon",
            Self::Macaron => "theme-macaron",
            Self::HighContrast => "theme-high-contrast",
            Self::Aero => "theme-aero",
        }
    }

    fn definition(self) -> &'static Definition {
        static BAMBOO: LazyLock<Definition> = LazyLock::new(|| {
            toml::from_str(include_str!("builtin/assets/bamboo.toml"))
                .expect("valid embedded Bamboo theme")
        });
        static RETRO: LazyLock<Definition> = LazyLock::new(|| {
            toml::from_str(include_str!("builtin/assets/retro.toml"))
                .expect("valid embedded Retro theme")
        });
        static NEON: LazyLock<Definition> = LazyLock::new(|| {
            toml::from_str(include_str!("builtin/assets/neon.toml"))
                .expect("valid embedded Neon theme")
        });
        static MACARON: LazyLock<Definition> = LazyLock::new(|| {
            toml::from_str(include_str!("builtin/assets/macaron.toml"))
                .expect("valid embedded Macaron theme")
        });
        static HIGH_CONTRAST: LazyLock<Definition> = LazyLock::new(|| {
            toml::from_str(include_str!("builtin/assets/high_contrast.toml"))
                .expect("valid embedded High Contrast theme")
        });
        static AERO: LazyLock<Definition> = LazyLock::new(|| {
            toml::from_str(include_str!("builtin/assets/aero.toml"))
                .expect("valid embedded Aero theme")
        });
        match self {
            Self::Bamboo => &BAMBOO,
            Self::Retro => &RETRO,
            Self::Neon => &NEON,
            Self::Macaron => &MACARON,
            Self::HighContrast => &HIGH_CONTRAST,
            Self::Aero => &AERO,
        }
    }

    fn palette(self, mode: Mode) -> Palette {
        self.definition().palette(mode)
    }
}

pub(super) fn load(theme: Theme, preference: ModePreference, platform: Mode) -> Loaded {
    let mode = preference.resolve(platform);
    Loaded {
        palette: theme.palette(mode),
        mode,
        source_path: None,
    }
}

/// Imported formats use Bamboo's defaults for status roles absent from their schemas.
pub(super) fn palette(mode: Mode) -> Palette {
    Theme::default().palette(mode)
}

#[cfg(test)]
mod tests;
