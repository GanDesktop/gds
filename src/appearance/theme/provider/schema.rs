//! Shared TOML schema for built-in and user-defined GDS palettes.

use std::{collections::BTreeMap, path::Path};

use serde::Deserialize;
use zgui::canvas::zgui_color::Color;

use super::{Error, Mode, Palette, color::hex_color};

pub const SCHEMA_VERSION: u32 = 1;

/// Self-description of a custom theme. Only the default name is required.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub name: String,
    pub schema_version: Option<u32>,
    pub author: Option<String>,
    #[serde(default)]
    pub i18n: BTreeMap<String, String>,
}

impl Metadata {
    pub fn display_name(&self, locale: &str) -> &str {
        self.i18n
            .iter()
            .find(|(language, name)| {
                language.eq_ignore_ascii_case(locale) && !name.trim().is_empty()
            })
            .map_or(&self.name, |(_, name)| name)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.name.trim().is_empty() {
            return Err(Error::invalid(
                "metadata.name",
                "a default theme name is required",
            ));
        }
        if self
            .schema_version
            .is_some_and(|version| version != SCHEMA_VERSION)
        {
            return Err(Error::invalid(
                "metadata.schema_version",
                "unsupported schema version",
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Definition {
    metadata: Option<Metadata>,
    light: Scheme,
    dark: Scheme,
}

impl Definition {
    pub(super) fn parse_custom(text: &str, path: &Path) -> Result<(Metadata, Self), Error> {
        let mut definition: Self = toml::from_str(text)
            .map_err(|error| Error::invalid(path.display().to_string(), error.to_string()))?;
        let metadata = definition
            .metadata
            .take()
            .ok_or_else(|| Error::invalid("metadata.name", "a default theme name is required"))?;
        metadata.validate()?;
        Ok((metadata, definition))
    }

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
    type Error = Error;
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
