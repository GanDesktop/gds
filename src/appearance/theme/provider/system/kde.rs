//! Read Plasma's effective colors from kdeglobals.
use std::path::Path;

use anti_webui::prelude::{Color, rgb};
use ini::Ini;

use super::super::{Loaded, builtin, color::hex_color};
use crate::appearance::theme::{Error, Mode, ModePreference, palette::Palette};

/// Read Plasma's effective colors, including the applied accent and custom edits.
///
/// # Errors
/// Rejects malformed INI, missing required color groups or invalid RGB values.
pub(super) fn parse(
    contents: &str,
    path: &Path,
    preference: ModePreference,
) -> Result<Loaded, Error> {
    let ini = parse_ini(contents, path)?;
    let mode = match preference {
        ModePreference::Light => Mode::Light,
        ModePreference::Dark => Mode::Dark,
        ModePreference::System => {
            let background: Color = required(&ini, "Colors:Window", "BackgroundNormal")?;
            // Match Plasma's Settings portal: qGray(windowBackground) < 192.
            // https://invent.kde.org/plasma/xdg-desktop-portal-kde/-/blob/master/src/settings.cpp
            let gray = (background.r * 11.0 + background.g * 16.0 + background.b * 5.0) / 32.0;
            if gray < 192.0 / 255.0 {
                Mode::Dark
            } else {
                Mode::Light
            }
        }
    };
    Ok(Loaded {
        palette: parse_kde(&ini, mode)?,
        mode,
        source_path: Some(path.to_owned()),
    })
}

/// # Errors
/// Returns the INI parse error with its source path.
fn parse_ini(contents: &str, path: &Path) -> Result<Ini, Error> {
    Ini::load_from_str(contents).map_err(|source| Error::Ini {
        path: path.to_owned(),
        source,
    })
}

/// # Errors
/// Rejects missing required KDE groups or malformed RGB components.
fn parse_kde(ini: &Ini, mode: Mode) -> Result<Palette, Error> {
    let mut palette = builtin::palette(mode);
    palette.sidebar = required(ini, "Colors:Window", "BackgroundNormal")?;
    palette.sidebar_foreground = required(ini, "Colors:Window", "ForegroundNormal")?;
    palette.content = required(ini, "Colors:View", "BackgroundNormal")?;
    palette.foreground = required(ini, "Colors:View", "ForegroundNormal")?;
    palette.selection = required(ini, "Colors:Selection", "BackgroundNormal")?;
    palette.selection_foreground = required(ini, "Colors:Selection", "ForegroundNormal")?;
    palette.primary = palette.selection;
    palette.primary_foreground = palette.selection_foreground;
    palette.surface =
        optional(ini, "Colors:Button", "BackgroundNormal")?.unwrap_or(palette.sidebar);
    palette.surface_foreground =
        optional(ini, "Colors:Button", "ForegroundNormal")?.unwrap_or(palette.sidebar_foreground);
    palette.muted_foreground =
        optional(ini, "Colors:View", "ForegroundInactive")?.unwrap_or(palette.foreground);
    palette.border =
        optional(ini, "Colors:Window", "ForegroundInactive")?.unwrap_or(palette.muted_foreground);
    palette.outline = palette.border;
    palette.panel = palette.surface;
    palette.panel_foreground = palette.surface_foreground;
    palette.tooltip =
        optional(ini, "Colors:Tooltip", "BackgroundNormal")?.unwrap_or(palette.sidebar);
    palette.tooltip_foreground =
        optional(ini, "Colors:Tooltip", "ForegroundNormal")?.unwrap_or(palette.sidebar_foreground);
    palette.link = optional(ini, "Colors:View", "ForegroundLink")?.unwrap_or(palette.primary);
    palette.terminal_fallback(mode);
    Ok(palette)
}

/// # Errors
/// Rejects a present named color if it is not valid RGB/ARGB data.
fn optional(ini: &Ini, section: &str, key: &str) -> Result<Option<Color>, Error> {
    ini.get_from(Some(section), key)
        .map(|value| named_color(value, &format!("{section}.{key}")))
        .transpose()
}

/// # Errors
/// Rejects a missing required named color or invalid color data.
fn required(ini: &Ini, section: &str, key: &str) -> Result<Color, Error> {
    optional(ini, section, key)?
        .ok_or_else(|| Error::invalid(format!("{section}.{key}"), "required color is missing"))
}

/// # Errors
/// Rejects unsupported color syntax, missing channels and values outside u8.
fn named_color(value: &str, field: &str) -> Result<Color, Error> {
    if value.trim().starts_with('#') {
        return hex_color(value, field, true);
    }
    let channels = value
        .split(',')
        .map(|channel| channel.trim().parse::<u8>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Error::invalid(field, format!("invalid RGB color {value:?}")))?;
    if !matches!(channels.len(), 3 | 4) {
        return Err(Error::invalid(field, "expected r,g,b or r,g,b,a"));
    }
    let color =
        (u32::from(channels[0]) << 16) | (u32::from(channels[1]) << 8) | u32::from(channels[2]);
    Ok(rgb(color))
}

#[cfg(test)]
mod tests;
