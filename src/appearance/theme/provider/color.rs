//! Color syntax shared by file-backed palette providers.
use gpui_kit::{Hsla, rgb};

use super::Error;

/// Parse an RGB color; Qt's optional leading alpha is intentionally discarded.
/// GDS controls opacity independently and keeps text/popover colors opaque.
///
/// # Errors
/// Rejects unsupported color formats or non-hexadecimal digits.
pub(super) fn hex_color(value: &str, field: &str, qt_argb: bool) -> Result<Hsla, Error> {
    let Some(hex) = value.trim().strip_prefix('#') else {
        return Err(Error::invalid(
            field,
            "expected a hexadecimal color starting with #",
        ));
    };
    let number = u32::from_str_radix(hex, 16)
        .map_err(|_| Error::invalid(field, format!("invalid hexadecimal color {value:?}")))?;
    let color = match hex.len() {
        3 => ((number >> 8) * 17) << 16 | (((number >> 4) & 15) * 17) << 8 | (number & 15) * 17,
        6 => number,
        8 if qt_argb => number & 0x00ff_ffff,
        _ => {
            return Err(Error::invalid(
                field,
                "expected #RGB, #RRGGBB or Qt #AARRGGBB",
            ));
        }
    };
    Ok(rgb(color).into())
}
