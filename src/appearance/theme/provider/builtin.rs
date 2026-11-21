//! Embedded TOML palettes, parsed once and independent of runtime files.
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use zgui::canvas::zgui_color::Color;

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
struct PaletteColor(Color);

impl TryFrom<String> for PaletteColor {
    type Error = super::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        hex_color(&value, "builtin color", false).map(Self)
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scheme {
    content: PaletteColor,
    sidebar: PaletteColor,
    foreground: PaletteColor,
    sidebar_foreground: PaletteColor,
    muted_foreground: PaletteColor,
    surface: PaletteColor,
    surface_foreground: PaletteColor,
    panel: PaletteColor,
    panel_foreground: PaletteColor,
    #[serde(default)]
    control: Option<PaletteColor>,
    #[serde(default)]
    control_foreground: Option<PaletteColor>,
    border: PaletteColor,
    outline: PaletteColor,
    shadow: PaletteColor,
    #[serde(default)]
    scrim: Option<PaletteColor>,
    primary: PaletteColor,
    primary_foreground: PaletteColor,
    selection: PaletteColor,
    selection_foreground: PaletteColor,
    tooltip: PaletteColor,
    tooltip_foreground: PaletteColor,
    link: PaletteColor,
    error: PaletteColor,
    error_foreground: PaletteColor,
    success: PaletteColor,
    success_foreground: PaletteColor,
    warning: PaletteColor,
    warning_foreground: PaletteColor,

    #[serde(default)]
    background: Option<PaletteColor>,
    #[serde(default)]
    on_background: Option<PaletteColor>,
    #[serde(default)]
    on_surface: Option<PaletteColor>,
    #[serde(default)]
    surface_dim: Option<PaletteColor>,
    #[serde(default)]
    surface_bright: Option<PaletteColor>,
    #[serde(default)]
    surface_container_lowest: Option<PaletteColor>,
    #[serde(default)]
    surface_container_low: Option<PaletteColor>,
    #[serde(default)]
    surface_container: Option<PaletteColor>,
    #[serde(default)]
    surface_container_high: Option<PaletteColor>,
    #[serde(default)]
    surface_container_highest: Option<PaletteColor>,
    #[serde(default)]
    surface_variant: Option<PaletteColor>,
    #[serde(default)]
    on_surface_variant: Option<PaletteColor>,
    #[serde(default)]
    surface_tint: Option<PaletteColor>,
    #[serde(default)]
    inverse_surface: Option<PaletteColor>,
    #[serde(default)]
    inverse_on_surface: Option<PaletteColor>,
    #[serde(default)]
    inverse_primary: Option<PaletteColor>,
    #[serde(default)]
    outline_variant: Option<PaletteColor>,
    #[serde(default)]
    source_color: Option<PaletteColor>,

    #[serde(default)]
    on_primary: Option<PaletteColor>,
    #[serde(default)]
    primary_container: Option<PaletteColor>,
    #[serde(default)]
    on_primary_container: Option<PaletteColor>,
    #[serde(default)]
    primary_fixed: Option<PaletteColor>,
    #[serde(default)]
    primary_fixed_dim: Option<PaletteColor>,
    #[serde(default)]
    on_primary_fixed: Option<PaletteColor>,
    #[serde(default)]
    on_primary_fixed_variant: Option<PaletteColor>,

    #[serde(default)]
    secondary: Option<PaletteColor>,
    #[serde(default)]
    on_secondary: Option<PaletteColor>,
    #[serde(default)]
    secondary_container: Option<PaletteColor>,
    #[serde(default)]
    on_secondary_container: Option<PaletteColor>,
    #[serde(default)]
    secondary_fixed: Option<PaletteColor>,
    #[serde(default)]
    secondary_fixed_dim: Option<PaletteColor>,
    #[serde(default)]
    on_secondary_fixed: Option<PaletteColor>,
    #[serde(default)]
    on_secondary_fixed_variant: Option<PaletteColor>,

    #[serde(default)]
    tertiary: Option<PaletteColor>,
    #[serde(default)]
    on_tertiary: Option<PaletteColor>,
    #[serde(default)]
    tertiary_container: Option<PaletteColor>,
    #[serde(default)]
    on_tertiary_container: Option<PaletteColor>,
    #[serde(default)]
    tertiary_fixed: Option<PaletteColor>,
    #[serde(default)]
    tertiary_fixed_dim: Option<PaletteColor>,
    #[serde(default)]
    on_tertiary_fixed: Option<PaletteColor>,
    #[serde(default)]
    on_tertiary_fixed_variant: Option<PaletteColor>,

    #[serde(default)]
    on_error: Option<PaletteColor>,
    #[serde(default)]
    error_container: Option<PaletteColor>,
    #[serde(default)]
    on_error_container: Option<PaletteColor>,
}

impl Scheme {
    fn into_palette(self, mode: Mode) -> Palette {
        let colors = self;
        let z = Color::TRANSPARENT;
        let mut palette = Palette {
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
            control: z,
            control_foreground: z,
            border: colors.border.0,
            outline: colors.outline.0,
            shadow: colors.shadow.0,
            scrim: z,
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
            background: z,
            on_background: z,
            on_surface: z,
            surface_dim: z,
            surface_bright: z,
            surface_container_lowest: z,
            surface_container_low: z,
            surface_container: z,
            surface_container_high: z,
            surface_container_highest: z,
            surface_variant: z,
            on_surface_variant: z,
            surface_tint: z,
            inverse_surface: z,
            inverse_on_surface: z,
            inverse_primary: z,
            outline_variant: z,
            source_color: z,
            on_primary: z,
            primary_container: z,
            on_primary_container: z,
            primary_fixed: z,
            primary_fixed_dim: z,
            on_primary_fixed: z,
            on_primary_fixed_variant: z,
            secondary: z,
            on_secondary: z,
            secondary_container: z,
            on_secondary_container: z,
            secondary_fixed: z,
            secondary_fixed_dim: z,
            on_secondary_fixed: z,
            on_secondary_fixed_variant: z,
            tertiary: z,
            on_tertiary: z,
            tertiary_container: z,
            on_tertiary_container: z,
            tertiary_fixed: z,
            tertiary_fixed_dim: z,
            on_tertiary_fixed: z,
            on_tertiary_fixed_variant: z,
            on_error: z,
            error_container: z,
            on_error_container: z,
        };
        palette.sync_material_defaults(mode);

        macro_rules! override_opt {
            ($($field:ident),+ $(,)?) => {
                $(
                    if let Some(value) = colors.$field {
                        palette.$field = value.0;
                    }
                )+
            };
        }

        override_opt!(
            scrim,
            background,
            on_background,
            on_surface,
            surface_dim,
            surface_bright,
            surface_container_lowest,
            surface_container_low,
            surface_container,
            surface_container_high,
            surface_container_highest,
            surface_variant,
            on_surface_variant,
            surface_tint,
            inverse_surface,
            inverse_on_surface,
            inverse_primary,
            outline_variant,
            source_color,
            on_primary,
            primary_container,
            on_primary_container,
            primary_fixed,
            primary_fixed_dim,
            on_primary_fixed,
            on_primary_fixed_variant,
            secondary,
            on_secondary,
            secondary_container,
            on_secondary_container,
            secondary_fixed,
            secondary_fixed_dim,
            on_secondary_fixed,
            on_secondary_fixed_variant,
            tertiary,
            on_tertiary,
            tertiary_container,
            on_tertiary_container,
            tertiary_fixed,
            tertiary_fixed_dim,
            on_tertiary_fixed,
            on_tertiary_fixed_variant,
            on_error,
            error_container,
            on_error_container,
        );

        palette.control = colors.control.map_or(
            match mode {
                Mode::Light => palette.surface_container_lowest,
                Mode::Dark => palette.surface_container_highest,
            },
            |value| value.0,
        );
        palette.control_foreground = colors
            .control_foreground
            .map_or(palette.on_surface, |value| value.0);

        palette
    }
}

#[cfg(test)]
mod tests;
