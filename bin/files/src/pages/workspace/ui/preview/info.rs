//! Value formatting for the preview panel's label/value rows.
//!
//! Formatting is pure so it can be checked without a window: the panel decides
//! which rows exist, these functions decide what a known value looks like.

use std::time::Duration;

use crate::entities::file::directory::Permissions;

use super::header::{LineEndings, PixelSize};

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * 60;

/// Pixel dimensions as `1920 × 1080`.
pub(super) fn dimensions(size: PixelSize) -> String {
    format!("{} × {}", size.width, size.height)
}

/// Reduced aspect ratio, or a decimal ratio when the reduced form is unwieldy.
pub(super) fn aspect_ratio(size: PixelSize) -> String {
    let divisor = gcd(size.width, size.height);
    let (width, height) = (size.width / divisor, size.height / divisor);
    if width <= 50 && height <= 50 {
        format!("{width}:{height}")
    } else {
        format!("{:.2}:1", f64::from(size.width) / f64::from(size.height))
    }
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// Media length as `m:ss` or `h:mm:ss`; very short clips keep one decimal.
pub(super) fn duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds == 0 {
        return format!("{:.1} s", duration.as_secs_f64());
    }
    if seconds >= SECONDS_PER_HOUR {
        format!(
            "{}:{:02}:{:02}",
            seconds / SECONDS_PER_HOUR,
            seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE,
            seconds % SECONDS_PER_MINUTE
        )
    } else {
        format!(
            "{}:{:02}",
            seconds / SECONDS_PER_MINUTE,
            seconds % SECONDS_PER_MINUTE
        )
    }
}

/// Sample rate in kHz when it divides cleanly, otherwise Hz.
pub(super) fn sample_rate(hertz: u32) -> String {
    if hertz % 1000 == 0 {
        format!("{} kHz", hertz / 1000)
    } else if hertz % 100 == 0 {
        format!("{:.1} kHz", f64::from(hertz) / 1000.)
    } else {
        format!("{:.2} kHz", f64::from(hertz) / 1000.)
    }
}

/// Bitrate in kbit/s, or bit/s for very low rates.
pub(super) fn bitrate(bits_per_second: u64) -> String {
    if bits_per_second >= 1000 {
        format!("{} kbit/s", (bits_per_second + 500) / 1000)
    } else {
        format!("{bits_per_second} bit/s")
    }
}

/// `rw-r--r-- · 0644`.
pub(super) fn permissions(permissions: &Permissions) -> String {
    format!("{} · {}", permissions.symbols(), permissions.octal())
}

/// Whether a line-ending style is shown as a literal or a localized word.
pub(super) fn line_endings(line_endings: LineEndings) -> LineEndingLabel {
    match line_endings {
        LineEndings::Lf => LineEndingLabel::Literal("LF"),
        LineEndings::Crlf => LineEndingLabel::Literal("CRLF"),
        LineEndings::Cr => LineEndingLabel::Literal("CR"),
        LineEndings::Mixed => LineEndingLabel::Message("details-mixed"),
    }
}

pub(super) enum LineEndingLabel {
    Literal(&'static str),
    Message(&'static str),
}

/// A counted value; `≥` marks a directory that hit the inspection bound.
pub(super) fn count(count: u32, capped: bool) -> String {
    if capped {
        format!("≥ {count}")
    } else {
        count.to_string()
    }
}
