//! Color syntax, CSS formatting, and blending helpers shared by palette providers.
use zgui::canvas::zgui_color::{Color, ColorSpace};

use super::Error;

pub(crate) fn rgb(color: u32) -> Color {
    Color::srgb_u8(
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
        0xff,
    )
}

pub(crate) fn color_css(color: Color) -> String {
    let srgb = color.to_space(ColorSpace::Srgb);
    let [r, g, b] = srgb
        .components()
        .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    let a = srgb.alpha().clamp(0.0, 1.0);
    if (a - 1.0).abs() <= 1e-4 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        let formatted = format!("{a:.3}");
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        format!("rgba({r}, {g}, {b}, {trimmed})")
    }
}

/// Parse an RGB color; KDE's optional leading alpha is intentionally discarded.
/// GDS controls opacity independently and keeps text/popover colors opaque.
///
/// # Errors
/// Rejects unsupported color formats or non-hexadecimal digits.
pub(super) fn hex_color(value: &str, field: &str, allow_argb: bool) -> Result<Color, Error> {
    let Some(hex) = value.trim().strip_prefix('#') else {
        return Err(Error::invalid(
            field,
            "expected a hexadecimal color starting with #",
        ));
    };
    let number = u32::from_str_radix(hex, 16)
        .map_err(|_| Error::invalid(field, format!("invalid hexadecimal color {value:?}")))?;
    let color = match hex.len() {
        3 => {
            (((number >> 8) * 17) << 16) | ((((number >> 4) & 15) * 17) << 8) | ((number & 15) * 17)
        }
        6 => number,
        8 if allow_argb => number & 0x00ff_ffff,
        _ => {
            return Err(Error::invalid(field, "expected #RGB, #RRGGBB or #AARRGGBB"));
        }
    };
    Ok(rgb(color))
}

/// Composition used when importing translucent desktop palette roles.
pub(crate) trait ColorExt {
    fn overlay(self, foreground: Self) -> Self;
    fn lightness(self) -> f32;
}

impl ColorExt for Color {
    fn overlay(self, foreground: Self) -> Self {
        let bg = self.to_space(ColorSpace::Srgb);
        let fg = foreground.to_space(ColorSpace::Srgb);
        let [br, bgc, bb] = bg.components();
        let [fr, fg_g, fb] = fg.components();
        let ba = bg.alpha().clamp(0.0, 1.0);
        let fa = fg.alpha().clamp(0.0, 1.0);
        let alpha = fa + ba * (1.0 - fa);
        if alpha <= 0.0 {
            return Self::TRANSPARENT;
        }
        let channel = |base: f32, front: f32| (front * fa + base * ba * (1.0 - fa)) / alpha;
        Self::srgb(channel(br, fr), channel(bgc, fg_g), channel(bb, fb), alpha)
    }

    fn lightness(self) -> f32 {
        let [r, g, b] = self.to_space(ColorSpace::Srgb).components();
        (r.max(g).max(b) + r.min(g).min(b)) * 0.5
    }
}
