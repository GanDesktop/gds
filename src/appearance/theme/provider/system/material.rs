//! Shared Material 3 role mapping for desktop shell exports.
use std::collections::HashMap;

use anti_webui::prelude::Color;

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

/// Map one resolved Material 3 scheme onto GDS's UI roles and initialize terminal fallback.
///
/// # Errors
/// Rejects a missing requested scheme, absent required roles, unpaired container/error
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
    if colors.contains_key("primary_container") || colors.contains_key("on_primary_container") {
        palette.selection = required(colors, "primary_container", provider)?;
        palette.selection_foreground = required(colors, "on_primary_container", provider)?;
    }
    palette.tooltip = palette.panel;
    palette.tooltip_foreground = palette.foreground;
    palette.link = palette.primary;
    if colors.contains_key("error") || colors.contains_key("on_error") {
        palette.error = required(colors, "error", provider)?;
        palette.error_foreground = required(colors, "on_error", provider)?;
    }
    palette.terminal_fallback(mode);
    Ok(palette)
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
