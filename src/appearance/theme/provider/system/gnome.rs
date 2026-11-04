//! Read the active GTK theme through its native style resolver.
use gpui_kit::{Hsla, Rgba, WindowAppearance};
use std::{collections::HashMap, process::Command};

use super::super::{Error, Loaded, Mode, ModePreference, builtin};

const READER: &str = include_str!("gnome/assets/read-theme.py");
const MUTED_OPACITY: f32 = 0.55;
const BORDER_OPACITY: f32 = 0.15;

/// # Errors
/// Reports missing Python/GTK bindings, display failures and incomplete GTK palettes.
pub(super) fn load(
    preference: ModePreference,
    platform: WindowAppearance,
) -> Result<Loaded, Error> {
    let requested = match preference {
        ModePreference::System => "system",
        ModePreference::Light => "light",
        ModePreference::Dark => "dark",
    };
    let platform = match ModePreference::System.resolve(platform) {
        Mode::Light => "light",
        Mode::Dark => "dark",
    };
    // GTK initialization and its display connection stay outside the GPUI process.
    let output = Command::new("python3")
        .args(["-c", READER, requested, platform])
        .output()
        .map_err(|error| Error::invalid("GTK theme reader", error.to_string()))?;
    if !output.status.success() {
        return Err(Error::invalid(
            "GTK theme reader",
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    parse(&output.stdout, preference)
}

fn parse(contents: &[u8], preference: ModePreference) -> Result<Loaded, Error> {
    let roles: HashMap<String, [f32; 4]> = serde_json::from_slice(contents)
        .map_err(|error| Error::invalid("GTK palette", error.to_string()))?;
    for (name, channels) in &roles {
        if channels
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(Error::invalid(name, "invalid GTK color channels"));
        }
    }
    let color = |name: &str| -> Option<Hsla> {
        roles
            .get(name)
            .map(|&[r, g, b, a]| Rgba { r, g, b, a }.into())
    };
    let required = |name: &str| {
        color(name).ok_or_else(|| Error::invalid(name, "GTK theme does not define this role"))
    };
    let window = required("theme_bg_color")?;
    let mode = match preference {
        ModePreference::Light => Mode::Light,
        ModePreference::Dark => Mode::Dark,
        ModePreference::System => {
            if window.l < 0.5 {
                Mode::Dark
            } else {
                Mode::Light
            }
        }
    };
    // Only business status roles absent from GTK retain the shared defaults.
    let mut palette = builtin::palette(mode);
    palette.content = required("theme_base_color")?;
    palette.foreground = required("theme_text_color")?;
    palette.sidebar = window;
    palette.sidebar_foreground = required("theme_fg_color")?;
    palette.surface = window;
    palette.surface_foreground = palette.sidebar_foreground;
    palette.panel = window;
    palette.panel_foreground = palette.sidebar_foreground;
    palette.primary = required("theme_selected_bg_color")?;
    palette.primary_foreground = required("theme_selected_fg_color")?;
    palette.selection = palette.primary;
    palette.selection_foreground = palette.primary_foreground;
    palette.muted_foreground = color("insensitive_fg_color")
        .map(|foreground| palette.content.blend(foreground))
        .unwrap_or_else(|| {
            palette
                .content
                .blend(palette.foreground.opacity(MUTED_OPACITY))
        });
    palette.border = color("borders").unwrap_or_else(|| {
        palette
            .content
            .blend(palette.foreground.opacity(BORDER_OPACITY))
    });
    palette.outline = palette.border;
    match (color("tooltip_bg_color"), color("tooltip_fg_color")) {
        (Some(background), Some(foreground)) => {
            palette.tooltip = background;
            palette.tooltip_foreground = foreground;
        }
        _ => {
            palette.tooltip = palette.panel;
            palette.tooltip_foreground = palette.panel_foreground;
        }
    }
    palette.link = palette.primary;
    palette.terminal_fallback(mode);
    Ok(Loaded {
        palette,
        mode,
        source_path: None,
    })
}

#[cfg(test)]
mod tests;
