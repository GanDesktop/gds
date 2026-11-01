use std::{collections::HashMap, path::Path};

use gpui_kit::Hsla;
use serde::Deserialize;

use super::super::{builtin, color::hex_color};
use crate::appearance::theme::{Error, Mode, ModePreference, palette::Palette};

#[derive(Deserialize)]
struct Export {
    // Only System needs this metadata; explicit choices accept any exported value.
    #[serde(default)]
    mode: Option<serde_json::Value>,
    colors: HashMap<String, HashMap<String, String>>,
}

impl Export {
    /// # Errors
    /// System requires an active mode named light or dark in the export.
    fn resolve_mode(&self, preference: ModePreference) -> Result<Mode, Error> {
        match preference {
            ModePreference::Light => Ok(Mode::Light),
            ModePreference::Dark => Ok(Mode::Dark),
            ModePreference::System => {
                match self.mode.as_ref().and_then(serde_json::Value::as_str) {
                    Some("light") => Ok(Mode::Light),
                    Some("dark") => Ok(Mode::Dark),
                    _ => Err(Error::invalid(
                        "mode",
                        "DMS active mode must be light or dark when following the system",
                    )),
                }
            }
        }
    }
}

pub(super) struct Parsed {
    pub palette: Palette,
    pub mode: Mode,
}

/// Resolve the requested scheme from a DMS export, without evaluating QML.
/// System uses the export's active mode; explicit choices override it.
///
/// # Errors
/// Rejects malformed JSON, invalid active mode when following System, an absent
/// requested scheme, missing roles or invalid colors. No other scheme is substituted.
pub(super) fn parse(
    contents: &str,
    path: &Path,
    preference: ModePreference,
) -> Result<Parsed, Error> {
    let export: Export = serde_json::from_str(contents).map_err(|source| Error::Json {
        path: path.to_owned(),
        source,
    })?;
    let mode = export.resolve_mode(preference)?;
    let name = match mode {
        Mode::Light => "light",
        Mode::Dark => "dark",
    };
    let colors = export.colors.get(name).ok_or_else(|| {
        Error::invalid(format!("colors.{name}"), "requested DMS scheme is missing")
    })?;
    let mut palette = builtin::palette(mode);
    palette.content = required(colors, "surface")?;
    palette.foreground = required(colors, "on_surface")?;
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
    palette.primary = required(colors, "primary")?;
    palette.primary_foreground = required(colors, "on_primary")?;
    palette.selection = palette.primary;
    palette.selection_foreground = palette.primary_foreground;
    if colors.contains_key("primary_container") || colors.contains_key("on_primary_container") {
        palette.selection = required(colors, "primary_container")?;
        palette.selection_foreground = required(colors, "on_primary_container")?;
    }
    palette.tooltip = palette.panel;
    palette.tooltip_foreground = palette.foreground;
    palette.link = palette.primary;
    if colors.contains_key("error") || colors.contains_key("on_error") {
        palette.error = required(colors, "error")?;
        palette.error_foreground = required(colors, "on_error")?;
    }
    Ok(Parsed { palette, mode })
}

/// # Errors
/// Returns an error when a present role is not a supported RGB color.
fn optional(colors: &HashMap<String, String>, role: &str) -> Result<Option<Hsla>, Error> {
    colors
        .get(role)
        .map(|value| hex_color(value, role, false))
        .transpose()
}

/// # Errors
/// Returns an error for a missing role or an invalid RGB color.
fn required(colors: &HashMap<String, String>, role: &str) -> Result<Hsla, Error> {
    optional(colors, role)?.ok_or_else(|| Error::invalid(role, "required DMS color is missing"))
}

#[cfg(test)]
mod tests;
