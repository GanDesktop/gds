use std::path::{Path, PathBuf};

use gpui_kit::{Hsla, rgb};
use ini::Ini;

use super::{Loaded, builtin, color::hex_color};
use crate::appearance::theme::{Error, Mode, ModePreference, palette::Palette};

// QPalette::ColorRole positions, not theme-specific array offsets.
const WINDOW_TEXT: usize = 0;
const BUTTON: usize = 1;
const MID: usize = 5;
const TEXT: usize = 6;
const BUTTON_TEXT: usize = 8;
const BASE: usize = 9;
const WINDOW: usize = 10;
const HIGHLIGHT: usize = 12;
const HIGHLIGHTED_TEXT: usize = 13;
const LINK: usize = 14;
const TOOLTIP_BASE: usize = 18;
const TOOLTIP_TEXT: usize = 19;
const PLACEHOLDER_TEXT: usize = 20;
const REQUIRED_ROLES: usize = TOOLTIP_TEXT + 1;

/// Resolve the palette actually selected by qtct, including relative paths.
///
/// # Errors
/// Rejects malformed INI, disabled custom palettes and empty palette pointers.
pub(super) fn palette_path(contents: &str, config: &Path, home: &Path) -> Result<PathBuf, Error> {
    let ini = parse_ini(contents, config)?;
    match ini
        .get_from(Some("Appearance"), "custom_palette")
        .map(str::trim)
    {
        Some("true" | "1") => {}
        None | Some("false" | "0") => return Err(Error::CustomPaletteDisabled(config.to_owned())),
        Some(value) => {
            return Err(Error::invalid(
                "Appearance.custom_palette",
                format!("invalid boolean {value:?}"),
            ));
        }
    }
    let value = ini
        .get_from(Some("Appearance"), "color_scheme_path")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::invalid("Appearance.color_scheme_path", "palette path is missing"))?;
    if value == "~" {
        return Ok(home.to_owned());
    }
    if let Some(relative) = value.strip_prefix("~/") {
        return Ok(home.join(relative));
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(config.parent().unwrap_or_else(|| Path::new(".")).join(path))
}

/// Parse qtct role arrays or the basic named groups of a KDE color scheme.
///
/// # Errors
/// Rejects unknown formats, malformed role arrays and invalid named colors.
pub(super) fn parse(contents: &str, path: &Path, mode: Mode) -> Result<Palette, Error> {
    let ini = parse_ini(contents, path)?;
    if ini.section(Some("ColorScheme")).is_some() {
        parse_qtct(&ini, mode)
    } else if ini.section(Some("Colors:Window")).is_some() {
        parse_kde(&ini, mode)
    } else {
        Err(Error::invalid(
            "palette format",
            "expected qtct [ColorScheme] or KDE [Colors:Window] groups",
        ))
    }
}

/// Read Plasma's effective colors, including the applied accent and custom edits.
///
/// # Errors
/// Rejects malformed INI, missing required color groups or invalid RGB values.
pub(super) fn parse_kde_system(
    contents: &str,
    path: &Path,
    preference: ModePreference,
) -> Result<Loaded, Error> {
    let ini = parse_ini(contents, path)?;
    let mode = match preference {
        ModePreference::Light => Mode::Light,
        ModePreference::Dark => Mode::Dark,
        ModePreference::System => {
            let background: gpui_kit::Rgba =
                required(&ini, "Colors:Window", "BackgroundNormal")?.into();
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
/// Rejects a missing active palette, incomplete arrays or invalid colors.
fn parse_qtct(ini: &Ini, mode: Mode) -> Result<Palette, Error> {
    let active = role_array(ini, "active_colors")?
        .ok_or_else(|| Error::invalid("ColorScheme.active_colors", "active palette is missing"))?;
    let disabled = role_array(ini, "disabled_colors")?;
    // Validate inactive data when present, but do not confuse it with disabled
    // widget state. The first bridge maps active and disabled text roles only.
    let _inactive = role_array(ini, "inactive_colors")?;
    let mut palette = builtin::palette(mode);
    palette.content = active[BASE];
    palette.sidebar = active[WINDOW];
    palette.foreground = active[TEXT];
    palette.sidebar_foreground = active[WINDOW_TEXT];
    palette.surface = active[BUTTON];
    palette.surface_foreground = active[BUTTON_TEXT];
    palette.panel = palette.surface;
    palette.panel_foreground = palette.surface_foreground;
    palette.border = active[MID];
    palette.outline = palette.border;
    // HighlightedText is paired with Highlight, not an independently supplied
    // Qt 6 Accent; keep this pair together for filled controls.
    palette.primary = active[HIGHLIGHT];
    palette.primary_foreground = active[HIGHLIGHTED_TEXT];
    palette.selection = active[HIGHLIGHT];
    palette.selection_foreground = active[HIGHLIGHTED_TEXT];
    palette.muted_foreground = disabled
        .as_ref()
        .map(|roles| roles[TEXT])
        .or_else(|| active.get(PLACEHOLDER_TEXT).copied())
        .unwrap_or(active[TEXT]);
    palette.tooltip = active[TOOLTIP_BASE];
    palette.tooltip_foreground = active[TOOLTIP_TEXT];
    palette.link = active[LINK];
    palette.terminal_fallback(mode);
    Ok(palette)
}

/// # Errors
/// Rejects a present array whose role count or color representation is invalid.
fn role_array(ini: &Ini, name: &str) -> Result<Option<Vec<Hsla>>, Error> {
    let Some(value) = ini.get_from(Some("ColorScheme"), name) else {
        return Ok(None);
    };
    let colors: Vec<Hsla> = value
        .split(',')
        .enumerate()
        .map(|(index, value)| hex_color(value, &format!("ColorScheme.{name}[{index}]"), true))
        .collect::<Result<_, _>>()?;
    if colors.len() < REQUIRED_ROLES {
        return Err(Error::invalid(
            format!("ColorScheme.{name}"),
            format!(
                "expected at least {REQUIRED_ROLES} Qt roles, got {}",
                colors.len()
            ),
        ));
    }
    Ok(Some(colors))
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
fn optional(ini: &Ini, section: &str, key: &str) -> Result<Option<Hsla>, Error> {
    ini.get_from(Some(section), key)
        .map(|value| named_color(value, &format!("{section}.{key}")))
        .transpose()
}

/// # Errors
/// Rejects a missing required named color or invalid color data.
fn required(ini: &Ini, section: &str, key: &str) -> Result<Hsla, Error> {
    optional(ini, section, key)?
        .ok_or_else(|| Error::invalid(format!("{section}.{key}"), "required color is missing"))
}

/// # Errors
/// Rejects unsupported color syntax, missing channels and values outside u8.
fn named_color(value: &str, field: &str) -> Result<Hsla, Error> {
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
    Ok(rgb(color).into())
}

#[cfg(test)]
mod tests;
