//! Color syntax shared by file-backed palette providers.
use anti_webui::prelude::{Color, rgb};

use super::Error;

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
        let alpha = foreground.a + self.a * (1.0 - foreground.a);
        if alpha == 0.0 {
            return Self::TRANSPARENT;
        }
        let channel = |base: f32, front: f32| {
            (front * foreground.a + base * self.a * (1.0 - foreground.a)) / alpha
        };
        Self::new(
            channel(self.r, foreground.r),
            channel(self.g, foreground.g),
            channel(self.b, foreground.b),
            alpha,
        )
    }
    fn lightness(self) -> f32 {
        (self.r.max(self.g).max(self.b) + self.r.min(self.g).min(self.b)) * 0.5
    }
}
