//! Embedded TOML palettes, parsed once and independent of runtime files.
use std::sync::LazyLock;

use gpui_kit::{Hsla, WindowAppearance};
use serde::{Deserialize, Serialize};

use super::{Loaded, Mode, ModePreference, Palette, color::hex_color};

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

pub(super) fn load(theme: Theme, preference: ModePreference, platform: WindowAppearance) -> Loaded {
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Definition {
    pub(super) metadata: Option<super::custom::Metadata>,
    light: Scheme,
    dark: Scheme,
}

impl Definition {
    pub(super) fn palette(&self, mode: Mode) -> Palette {
        let mut palette = match mode {
            Mode::Light => self.light,
            Mode::Dark => self.dark,
        }
        .into_palette(mode);
        palette.terminal_fallback(mode);
        palette
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(try_from = "String")]
struct Color(Hsla);

impl TryFrom<String> for Color {
    type Error = super::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        hex_color(&value, "builtin color", false).map(Self)
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scheme {
    content: Color,
    sidebar: Color,
    foreground: Color,
    sidebar_foreground: Color,
    muted_foreground: Color,
    surface: Color,
    surface_foreground: Color,
    panel: Color,
    panel_foreground: Color,
    border: Color,
    outline: Color,
    shadow: Color,
    primary: Color,
    primary_foreground: Color,
    selection: Color,
    selection_foreground: Color,
    tooltip: Color,
    tooltip_foreground: Color,
    link: Color,
    error: Color,
    error_foreground: Color,
    success: Color,
    success_foreground: Color,
    warning: Color,
    warning_foreground: Color,
}

impl Scheme {
    fn into_palette(self, mode: Mode) -> Palette {
        let colors = self;
        Palette {
            terminal: crate::appearance::theme::TerminalColors::fallback(mode),
            content: colors.content.0,
            sidebar: colors.sidebar.0,
            foreground: colors.foreground.0,
            sidebar_foreground: colors.sidebar_foreground.0,
            muted_foreground: colors.muted_foreground.0,
            surface: colors.surface.0,
            surface_foreground: colors.surface_foreground.0,
            panel: colors.panel.0,
            panel_foreground: colors.panel_foreground.0,
            border: colors.border.0,
            outline: colors.outline.0,
            shadow: colors.shadow.0,
            primary: colors.primary.0,
            primary_foreground: colors.primary_foreground.0,
            selection: colors.selection.0,
            selection_foreground: colors.selection_foreground.0,
            tooltip: colors.tooltip.0,
            tooltip_foreground: colors.tooltip_foreground.0,
            link: colors.link.0,
            error: colors.error.0,
            error_foreground: colors.error_foreground.0,
            success: colors.success.0,
            success_foreground: colors.success_foreground.0,
            warning: colors.warning.0,
            warning_foreground: colors.warning_foreground.0,
        }
    }
}

#[cfg(test)]
mod tests;
