//! Pure-Rust GNOME appearance resolution via Portal, Adwaita defaults, and GTK 3/4 config files.
#[cfg(test)]
mod tests;

use directories::BaseDirs;
use ini::Ini;
use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    path::Path,
};
use zgui::canvas::zgui_color::{Color, ColorSpace};

use super::super::{
    Error, Loaded, Mode, ModePreference, builtin,
    color::{ColorExt as _, rgb},
};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use crate::appearance::portal::Snapshot as PortalSnapshot;

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct PortalSnapshot {
    color_scheme: Option<Mode>,
    accent_color: Option<Color>,
    contrast: crate::appearance::theme::Contrast,
    primed: bool,
}

const MUTED_OPACITY: f32 = 0.55;
const BORDER_OPACITY: f32 = 0.15;
const MAX_COLOR_DEPTH: usize = 16;

/// Standard Adwaita named palette colors available to `@define-color` references in user CSS.
const ADWAITA_COLORS: &[(&str, u32)] = &[
    ("blue_1", 0x99c1f1),
    ("blue_2", 0x62a0ea),
    ("blue_3", 0x3584e4),
    ("blue_4", 0x1c71d8),
    ("blue_5", 0x1a5fb4),
    ("green_1", 0x8ff0a4),
    ("green_2", 0x57e389),
    ("green_3", 0x33d17a),
    ("green_4", 0x2ec27e),
    ("green_5", 0x26a269),
    ("yellow_1", 0xf9f06b),
    ("yellow_2", 0xf8e45c),
    ("yellow_3", 0xf6d32d),
    ("yellow_4", 0xf5c211),
    ("yellow_5", 0xe5a50a),
    ("orange_1", 0xffbe6f),
    ("orange_2", 0xffa348),
    ("orange_3", 0xff7800),
    ("orange_4", 0xe66100),
    ("orange_5", 0xc64600),
    ("red_1", 0xf66151),
    ("red_2", 0xed333b),
    ("red_3", 0xe01b24),
    ("red_4", 0xc01c28),
    ("red_5", 0xa51d2d),
    ("purple_1", 0xdc8add),
    ("purple_2", 0xc061cb),
    ("purple_3", 0x9141ac),
    ("purple_4", 0x813d9c),
    ("purple_5", 0x613583),
    ("brown_1", 0xcdab8f),
    ("brown_2", 0xb5835a),
    ("brown_3", 0x986a44),
    ("brown_4", 0x865e3c),
    ("brown_5", 0x63452c),
    ("light_1", 0xffffff),
    ("light_2", 0xf6f5f4),
    ("light_3", 0xdeddda),
    ("light_4", 0xc0bfbc),
    ("light_5", 0x9a9996),
    ("dark_1", 0x77767b),
    ("dark_2", 0x5e5c64),
    ("dark_3", 0x3d3846),
    ("dark_4", 0x241f31),
    ("dark_5", 0x000000),
];

struct BaseRoles {
    window_bg: Color,
    window_fg: Color,
    view_bg: Color,
    view_fg: Color,
    sidebar_bg: Color,
    sidebar_fg: Color,
    popover_bg: Color,
    popover_fg: Color,
    accent_bg: Color,
    accent_fg: Color,
}

impl BaseRoles {
    fn for_mode(mode: Mode, accent_override: Option<Color>) -> Self {
        let mut roles = match mode {
            Mode::Light => Self {
                window_bg: rgb(0xfafafa),
                window_fg: rgb(0x323232),
                view_bg: rgb(0xffffff),
                view_fg: rgb(0x333333),
                sidebar_bg: rgb(0xebebeb),
                sidebar_fg: rgb(0x323232),
                popover_bg: rgb(0xffffff),
                popover_fg: rgb(0x333333),
                accent_bg: rgb(0x3584e4),
                accent_fg: rgb(0xffffff),
            },
            Mode::Dark => Self {
                window_bg: rgb(0x242424),
                window_fg: rgb(0xffffff),
                view_bg: rgb(0x1e1e1e),
                view_fg: rgb(0xffffff),
                sidebar_bg: rgb(0x303030),
                sidebar_fg: rgb(0xffffff),
                popover_bg: rgb(0x383838),
                popover_fg: rgb(0xffffff),
                accent_bg: rgb(0x3584e4),
                accent_fg: rgb(0xffffff),
            },
        };
        if let Some(accent) = accent_override {
            roles.accent_bg = accent;
            roles.accent_fg = contrasting_foreground(accent);
        }
        roles
    }

    fn seed_colors(&self) -> HashMap<String, Color> {
        let mut colors = HashMap::with_capacity(ADWAITA_COLORS.len() + 20);
        for &(name, hex) in ADWAITA_COLORS {
            colors.insert(name.to_owned(), rgb(hex));
        }
        colors.insert("window_bg_color".into(), self.window_bg);
        colors.insert("window_fg_color".into(), self.window_fg);
        colors.insert("view_bg_color".into(), self.view_bg);
        colors.insert("view_fg_color".into(), self.view_fg);
        colors.insert("sidebar_bg_color".into(), self.sidebar_bg);
        colors.insert("sidebar_fg_color".into(), self.sidebar_fg);
        colors.insert("headerbar_bg_color".into(), self.window_bg);
        colors.insert("headerbar_fg_color".into(), self.window_fg);
        colors.insert("popover_bg_color".into(), self.popover_bg);
        colors.insert("popover_fg_color".into(), self.popover_fg);
        colors.insert("accent_bg_color".into(), self.accent_bg);
        colors.insert("accent_fg_color".into(), self.accent_fg);
        colors.insert("theme_bg_color".into(), self.window_bg);
        colors.insert("theme_fg_color".into(), self.window_fg);
        colors.insert("theme_base_color".into(), self.view_bg);
        colors.insert("theme_text_color".into(), self.view_fg);
        colors.insert("theme_selected_bg_color".into(), self.accent_bg);
        colors.insert("theme_selected_fg_color".into(), self.accent_fg);
        colors
    }
}

fn contrasting_foreground(accent: Color) -> Color {
    let [r, g, b] = accent.to_space(ColorSpace::Srgb).components();
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if luminance > 0.62 {
        rgb(0x1e1e1e)
    } else {
        rgb(0xffffff)
    }
}

/// # Errors
/// Reports unreadable GTK configuration files; missing files fall back to Adwaita and Portal defaults.
pub(super) fn load(
    preference: ModePreference,
    platform: Mode,
    dirs: &BaseDirs,
) -> Result<Loaded, Error> {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    let portal = crate::appearance::portal::snapshot();
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    let portal = PortalSnapshot::default();
    resolve(preference, platform, portal, dirs.config_dir())
}

fn resolve(
    preference: ModePreference,
    platform: Mode,
    portal: PortalSnapshot,
    config_dir: &Path,
) -> Result<Loaded, Error> {
    let prefer_dark = prefer_dark_theme(config_dir)?;
    let explicit_or_session_mode = match preference {
        ModePreference::Light => Some(Mode::Light),
        ModePreference::Dark => Some(Mode::Dark),
        ModePreference::System => portal
            .color_scheme
            .or_else(|| prefer_dark.map(|dark| if dark { Mode::Dark } else { Mode::Light })),
    };
    let initial_mode = explicit_or_session_mode.unwrap_or_else(|| preference.resolve(platform));
    let base = BaseRoles::for_mode(initial_mode, portal.accent_color);
    let mut colors = base.seed_colors();
    let mut overridden = HashSet::new();

    for relative in ["gtk-3.0/gtk.css", "gtk-4.0/gtk.css"] {
        let path = config_dir.join(relative);
        if let Some(css) = read_optional(&path)? {
            parse_gtk_css(&css, &mut colors, &mut overridden);
        }
    }

    let pick = |modern: &str, legacy: &str, default: Color| -> Color {
        if overridden.contains(modern) {
            colors[modern]
        } else if overridden.contains(legacy) {
            colors[legacy]
        } else {
            default
        }
    };

    let window_overridden =
        overridden.contains("window_bg_color") || overridden.contains("theme_bg_color");
    let window_fg_overridden =
        overridden.contains("window_fg_color") || overridden.contains("theme_fg_color");
    let window = pick("window_bg_color", "theme_bg_color", base.window_bg);
    let window_fg = pick("window_fg_color", "theme_fg_color", base.window_fg);

    let mode = match (explicit_or_session_mode, window_overridden) {
        (Some(mode), _) => mode,
        (None, true) => {
            if window.lightness() < 0.5 {
                Mode::Dark
            } else {
                Mode::Light
            }
        }
        (None, false) => initial_mode,
    };

    let sidebar = if overridden.contains("sidebar_bg_color") {
        colors["sidebar_bg_color"]
    } else if window_overridden {
        window
    } else {
        base.sidebar_bg
    };
    let sidebar_fg = if overridden.contains("sidebar_fg_color") {
        colors["sidebar_fg_color"]
    } else if window_fg_overridden {
        window_fg
    } else {
        base.sidebar_fg
    };
    let panel = if overridden.contains("popover_bg_color") {
        colors["popover_bg_color"]
    } else if window_overridden {
        window
    } else {
        base.popover_bg
    };
    let panel_fg = if overridden.contains("popover_fg_color") {
        colors["popover_fg_color"]
    } else if window_fg_overridden {
        window_fg
    } else {
        base.popover_fg
    };

    let accent_bg_overridden =
        overridden.contains("accent_bg_color") || overridden.contains("theme_selected_bg_color");
    let accent_fg_overridden =
        overridden.contains("accent_fg_color") || overridden.contains("theme_selected_fg_color");
    let primary = pick("accent_bg_color", "theme_selected_bg_color", base.accent_bg);
    let primary_fg = if accent_fg_overridden {
        pick("accent_fg_color", "theme_selected_fg_color", base.accent_fg)
    } else if accent_bg_overridden {
        contrasting_foreground(primary)
    } else {
        base.accent_fg
    };

    // Only business status roles absent from GTK/Adwaita retain the shared defaults.
    let mut palette = builtin::palette(mode);
    palette.content = pick("view_bg_color", "theme_base_color", base.view_bg);
    palette.foreground = pick("view_fg_color", "theme_text_color", base.view_fg);
    palette.sidebar = sidebar;
    palette.sidebar_foreground = sidebar_fg;
    palette.surface = window;
    palette.surface_foreground = window_fg;
    palette.panel = panel;
    palette.panel_foreground = panel_fg;
    palette.primary = primary;
    palette.primary_foreground = primary_fg;
    palette.selection = primary;
    palette.selection_foreground = primary_fg;
    palette.muted_foreground = if overridden.contains("insensitive_fg_color") {
        palette.content.overlay(colors["insensitive_fg_color"])
    } else {
        palette
            .content
            .overlay(palette.foreground.with_alpha(MUTED_OPACITY))
    };
    palette.border = if overridden.contains("borders") {
        palette.content.overlay(colors["borders"])
    } else {
        palette
            .content
            .overlay(palette.foreground.with_alpha(BORDER_OPACITY))
    };
    palette.outline = palette.border;
    match (
        overridden.contains("tooltip_bg_color"),
        overridden.contains("tooltip_fg_color"),
    ) {
        (true, true) => {
            palette.tooltip = colors["tooltip_bg_color"];
            palette.tooltip_foreground = colors["tooltip_fg_color"];
        }
        _ => {
            palette.tooltip = palette.panel;
            palette.tooltip_foreground = palette.panel_foreground;
        }
    }
    palette.link = palette.primary;
    palette.sync_material_defaults(mode);
    palette.terminal_fallback(mode);
    Ok(Loaded {
        palette,
        mode,
        source_path: None,
    })
}

fn read_optional(path: &Path) -> Result<Option<String>, Error> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Read {
            path: path.to_owned(),
            source,
        }),
    }
}

fn prefer_dark_theme(config_dir: &Path) -> Result<Option<bool>, Error> {
    let mut prefer_dark = None;
    for relative in ["gtk-3.0/settings.ini", "gtk-4.0/settings.ini"] {
        let path = config_dir.join(relative);
        let Some(contents) = read_optional(&path)? else {
            continue;
        };
        let ini = Ini::load_from_str(&contents).map_err(|source| Error::Ini {
            path: path.clone(),
            source,
        })?;
        let Some(raw) = ini.get_from(Some("Settings"), "gtk-application-prefer-dark-theme") else {
            continue;
        };
        if let Some(parsed) = parse_ini_bool(raw) {
            prefer_dark = Some(parsed);
        } else {
            tracing::debug!(
                path = %path.display(),
                value = raw.trim(),
                "ignoring unrecognized gtk-application-prefer-dark-theme value"
            );
        }
    }
    Ok(prefer_dark)
}

fn parse_ini_bool(value: &str) -> Option<bool> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("1")
        || trimmed.eq_ignore_ascii_case("true")
        || trimmed.eq_ignore_ascii_case("yes")
    {
        Some(true)
    } else if trimmed.eq_ignore_ascii_case("0")
        || trimmed.eq_ignore_ascii_case("false")
        || trimmed.eq_ignore_ascii_case("no")
    {
        Some(false)
    } else {
        None
    }
}

fn parse_gtk_css(css: &str, colors: &mut HashMap<String, Color>, overridden: &mut HashSet<String>) {
    let stripped = strip_css_comments(css);
    let mut cursor = stripped.as_str();
    while let Some(pos) = cursor.find("@define-color") {
        let after_kw = &cursor[pos + "@define-color".len()..];
        let trimmed = after_kw.trim_start();
        if after_kw.len() == trimmed.len() {
            cursor = after_kw;
            continue;
        }
        let Some(semi) = trimmed.find(';') else {
            break;
        };
        let directive = trimmed[..semi].trim();
        cursor = &trimmed[semi + 1..];
        let Some((name, expr)) = split_first_token(directive) else {
            continue;
        };
        if !is_css_ident(name) {
            continue;
        }
        if let Some(color) = parse_color_expr(expr, colors, 0) {
            colors.insert(name.to_owned(), color);
            overridden.insert(name.to_owned());
        } else {
            tracing::debug!(
                name,
                expr,
                "skipping unsupported GTK @define-color expression"
            );
        }
    }
}

fn strip_css_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("*/") else {
            return out;
        };
        rest = &after_start[end + 2..];
    }
    out.push_str(rest);
    out
}

fn split_first_token(input: &str) -> Option<(&str, &str)> {
    let input = input.trim();
    let split = input.find(char::is_whitespace)?;
    let (head, tail) = input.split_at(split);
    Some((head, tail.trim()))
}

fn is_css_ident(token: &str) -> bool {
    !token.is_empty()
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn parse_color_expr(expr: &str, colors: &HashMap<String, Color>, depth: usize) -> Option<Color> {
    if depth >= MAX_COLOR_DEPTH {
        return None;
    }
    let expr = expr.trim();
    if expr.is_empty() {
        return None;
    }
    if let Some(name) = expr.strip_prefix('@') {
        return is_css_ident(name)
            .then(|| colors.get(name).copied())
            .flatten();
    }
    if let Some(args) = strip_call(expr, "alpha") {
        let (inner, factor) = split_last_top_level_comma(args)?;
        let color = parse_color_expr(inner, colors, depth + 1)?;
        let factor = parse_alpha(factor)?;
        return Some(color.with_alpha((color.alpha() * factor).clamp(0.0, 1.0)));
    }
    parse_css_color_literal(expr)
}

fn parse_css_color_literal(expr: &str) -> Option<Color> {
    let parsed = color::parse_color(expr).ok()?;
    let [r, g, b, a] = parsed.to_alpha_color::<color::Srgb>().components;
    (r.is_finite() && g.is_finite() && b.is_finite() && a.is_finite()).then(|| {
        Color::srgb(
            normalize_srgb_channel(r),
            normalize_srgb_channel(g),
            normalize_srgb_channel(b),
            normalize_srgb_channel(a),
        )
    })
}

/// Align 8-bit channels scaled by `color`'s `*(1.0 / 255.0)` reciprocal multiply with
/// `Color::srgb_u8`'s `/ 255.0` division while keeping arbitrary fractional values intact.
fn normalize_srgb_channel(value: f32) -> f32 {
    let clamped = value.clamp(0.0, 1.0);
    let exact_u8 = (clamped * 255.0).round() / 255.0;
    if (clamped - exact_u8).abs() <= f32::EPSILON {
        exact_u8
    } else {
        clamped
    }
}

fn strip_call<'a>(expr: &'a str, name: &str) -> Option<&'a str> {
    let prefix = expr.get(..name.len())?;
    if !prefix.eq_ignore_ascii_case(name) {
        return None;
    }
    let rest = expr[name.len()..].trim_start();
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
    Some(inner)
}

fn split_last_top_level_comma(args: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (index, ch) in args.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                return Some((args[..index].trim(), args[index + 1..].trim()));
            }
            _ => {}
        }
    }
    None
}

fn parse_alpha(token: &str) -> Option<f32> {
    let token = token.trim();
    if let Some(pct) = token.strip_suffix('%') {
        let value: f32 = pct.trim().parse().ok()?;
        value.is_finite().then(|| (value / 100.0).clamp(0.0, 1.0))
    } else {
        let value: f32 = token.parse().ok()?;
        value.is_finite().then(|| value.clamp(0.0, 1.0))
    }
}
