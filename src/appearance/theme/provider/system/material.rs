//! Shared Material 3 role mapping for desktop shell exports (DMS and Noctalia).
use std::collections::HashMap;

use zgui::canvas::zgui_color::Color;

use super::super::{builtin, color::hex_color};
use crate::appearance::theme::{Error, Mode, ModePreference, palette::Palette};

pub(super) struct Parsed {
    pub palette: Palette,
    pub mode: Mode,
}

pub(super) const fn scheme_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Light => "light",
        Mode::Dark => "dark",
    }
}

/// # Errors
/// Following `System` requires an active mode named `light` or `dark` in the export.
pub(super) fn resolve_mode(
    active: Option<&str>,
    preference: ModePreference,
    provider: &str,
) -> Result<Mode, Error> {
    match preference {
        ModePreference::Light => Ok(Mode::Light),
        ModePreference::Dark => Ok(Mode::Dark),
        ModePreference::System => match active {
            Some("light") => Ok(Mode::Light),
            Some("dark") => Ok(Mode::Dark),
            _ => Err(Error::invalid(
                "mode",
                format!("{provider} active mode must be light or dark when following the system"),
            )),
        },
    }
}

/// Map one resolved Material 3 scheme onto GDS's semantic and aligned Material roles, and
/// initialize terminal fallback.
///
/// # Errors
/// Rejects a missing requested scheme, absent required roles, unpaired container/accent/error
/// roles, or malformed hexadecimal colors.
pub(super) fn palette(
    schemes: &HashMap<String, HashMap<String, String>>,
    mode: Mode,
    provider: &str,
) -> Result<Palette, Error> {
    let name = scheme_name(mode);
    let colors = schemes.get(name).ok_or_else(|| {
        Error::invalid(
            format!("colors.{name}"),
            format!("requested {provider} scheme is missing"),
        )
    })?;
    let mut palette = builtin::palette(mode);
    palette.content = required(colors, "surface", provider)?;
    palette.foreground = required(colors, "on_surface", provider)?;
    palette.sidebar = optional(colors, "surface_container_low")?.unwrap_or(palette.content);
    palette.sidebar_foreground = palette.foreground;
    palette.surface = optional(colors, "surface_container")?.unwrap_or(palette.content);
    palette.surface_foreground = palette.foreground;
    palette.panel = optional(colors, "surface_container_high")?.unwrap_or(palette.surface);
    palette.panel_foreground = palette.foreground;
    palette.muted_foreground =
        optional(colors, "on_surface_variant")?.unwrap_or(palette.foreground);
    palette.border = optional(colors, "outline_variant")?
        .or(optional(colors, "outline")?)
        .unwrap_or(palette.muted_foreground);
    palette.outline = optional(colors, "outline")?.unwrap_or(palette.border);
    palette.shadow = optional(colors, "shadow")?.unwrap_or(palette.shadow);
    palette.primary = required(colors, "primary", provider)?;
    palette.primary_foreground = required(colors, "on_primary", provider)?;
    palette.selection = palette.primary;
    palette.selection_foreground = palette.primary_foreground;
    if let Some((container, on_container)) = optional_pair(
        colors,
        "primary_container",
        "on_primary_container",
        provider,
    )? {
        palette.selection = container;
        palette.selection_foreground = on_container;
    }
    palette.tooltip = palette.panel;
    palette.tooltip_foreground = palette.foreground;
    palette.link = palette.primary;
    if let Some((err, on_err)) = optional_pair(colors, "error", "on_error", provider)? {
        palette.error = err;
        palette.error_foreground = on_err;
    }

    // Seed all Material 3 aligned defaults from this palette's imported base roles before
    // overlaying explicit Material tokens present in the export.
    palette.sync_material_defaults(mode);

    if let Some((bg, on_bg)) = optional_pair(colors, "background", "on_background", provider)? {
        palette.background = bg;
        palette.on_background = on_bg;
    }
    palette.on_surface = palette.foreground;
    if let Some(c) = optional(colors, "surface_dim")? {
        palette.surface_dim = c;
    }
    if let Some(c) = optional(colors, "surface_bright")? {
        palette.surface_bright = c;
    }
    if let Some(c) = optional(colors, "surface_container_lowest")? {
        palette.surface_container_lowest = c;
    }
    if let Some(c) = optional(colors, "surface_container_low")? {
        palette.surface_container_low = c;
    }
    if let Some(c) = optional(colors, "surface_container")? {
        palette.surface_container = c;
    }
    if let Some(c) = optional(colors, "surface_container_high")? {
        palette.surface_container_high = c;
    }
    if let Some(c) = optional(colors, "surface_container_highest")? {
        palette.surface_container_highest = c;
    }
    if let Some(c) = optional(colors, "surface_variant")? {
        palette.surface_variant = c;
    }
    if let Some(c) = optional(colors, "on_surface_variant")? {
        palette.on_surface_variant = c;
    }
    if let Some(c) = optional(colors, "surface_tint")? {
        palette.surface_tint = c;
    }
    if let Some((inv, on_inv)) =
        optional_pair(colors, "inverse_surface", "inverse_on_surface", provider)?
    {
        palette.inverse_surface = inv;
        palette.inverse_on_surface = on_inv;
    }
    if let Some(c) = optional(colors, "inverse_primary")? {
        palette.inverse_primary = c;
    }
    if let Some(c) = optional(colors, "outline_variant")? {
        palette.outline_variant = c;
    }
    if let Some(c) = optional(colors, "scrim")? {
        palette.scrim = c;
    }
    if let Some(c) = optional(colors, "source_color")? {
        palette.source_color = c;
    }

    if let Some(c) = optional(colors, "primary_fixed")? {
        palette.primary_fixed = c;
    }
    if let Some(c) = optional(colors, "primary_fixed_dim")? {
        palette.primary_fixed_dim = c;
    }
    if let Some(c) = optional(colors, "on_primary_fixed")? {
        palette.on_primary_fixed = c;
    }
    if let Some(c) = optional(colors, "on_primary_fixed_variant")? {
        palette.on_primary_fixed_variant = c;
    }

    if let Some((sec, on_sec)) = optional_pair(colors, "secondary", "on_secondary", provider)? {
        palette.secondary = sec;
        palette.on_secondary = on_sec;
        palette.secondary_fixed_dim = sec;
        palette.on_secondary_fixed_variant = sec;
    }
    if let Some((sec_c, on_sec_c)) = optional_pair(
        colors,
        "secondary_container",
        "on_secondary_container",
        provider,
    )? {
        palette.secondary_container = sec_c;
        palette.on_secondary_container = on_sec_c;
        palette.secondary_fixed = sec_c;
        palette.on_secondary_fixed = on_sec_c;
    }
    if let Some(c) = optional(colors, "secondary_fixed")? {
        palette.secondary_fixed = c;
    }
    if let Some(c) = optional(colors, "secondary_fixed_dim")? {
        palette.secondary_fixed_dim = c;
    }
    if let Some(c) = optional(colors, "on_secondary_fixed")? {
        palette.on_secondary_fixed = c;
    }
    if let Some(c) = optional(colors, "on_secondary_fixed_variant")? {
        palette.on_secondary_fixed_variant = c;
    }

    if let Some((ter, on_ter)) = optional_pair(colors, "tertiary", "on_tertiary", provider)? {
        palette.tertiary = ter;
        palette.on_tertiary = on_ter;
        palette.tertiary_fixed_dim = ter;
        palette.on_tertiary_fixed_variant = ter;
    }
    if let Some((ter_c, on_ter_c)) = optional_pair(
        colors,
        "tertiary_container",
        "on_tertiary_container",
        provider,
    )? {
        palette.tertiary_container = ter_c;
        palette.on_tertiary_container = on_ter_c;
        palette.tertiary_fixed = ter_c;
        palette.on_tertiary_fixed = on_ter_c;
    }
    if let Some(c) = optional(colors, "tertiary_fixed")? {
        palette.tertiary_fixed = c;
    }
    if let Some(c) = optional(colors, "tertiary_fixed_dim")? {
        palette.tertiary_fixed_dim = c;
    }
    if let Some(c) = optional(colors, "on_tertiary_fixed")? {
        palette.on_tertiary_fixed = c;
    }
    if let Some(c) = optional(colors, "on_tertiary_fixed_variant")? {
        palette.on_tertiary_fixed_variant = c;
    }

    if let Some((err_c, on_err_c)) =
        optional_pair(colors, "error_container", "on_error_container", provider)?
    {
        palette.error_container = err_c;
        palette.on_error_container = on_err_c;
    }

    palette.control = match mode {
        Mode::Light => palette.surface_container_lowest,
        Mode::Dark => palette.surface_container_highest,
    };
    palette.control_foreground = palette.on_surface;

    palette.terminal_fallback(mode);
    Ok(palette)
}

/// # Errors
/// Returns an error when only one side of a contrast pair is present or either side is invalid.
fn optional_pair(
    colors: &HashMap<String, String>,
    fill_role: &str,
    ink_role: &str,
    provider: &str,
) -> Result<Option<(Color, Color)>, Error> {
    if colors.contains_key(fill_role) || colors.contains_key(ink_role) {
        Ok(Some((
            required(colors, fill_role, provider)?,
            required(colors, ink_role, provider)?,
        )))
    } else {
        Ok(None)
    }
}

/// # Errors
/// Returns an error when a present role is not a supported RGB color.
fn optional(colors: &HashMap<String, String>, role: &str) -> Result<Option<Color>, Error> {
    colors
        .get(role)
        .map(|value| hex_color(value, role, false))
        .transpose()
}

/// # Errors
/// Returns an error for a missing role or an invalid RGB color.
fn required(colors: &HashMap<String, String>, role: &str, provider: &str) -> Result<Color, Error> {
    optional(colors, role)?
        .ok_or_else(|| Error::invalid(role, format!("required {provider} color is missing")))
}
