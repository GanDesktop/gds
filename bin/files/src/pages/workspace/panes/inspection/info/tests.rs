use std::time::Duration;

use super::{
    LineEndingLabel, aspect_ratio, bitrate, count, dimensions, duration, line_endings, permissions,
    sample_rate,
};
use crate::entities::file::directory::Permissions;
use gds::service::file_inspection::{LineEndings, PixelSize};

#[test]
fn aspect_ratio_rejects_zero_dimensions() {
    assert_eq!(
        aspect_ratio(PixelSize {
            width: 0,
            height: 0,
        }),
        None
    );
    assert_eq!(
        aspect_ratio(PixelSize {
            width: 1920,
            height: 0,
        }),
        None
    );
    assert_eq!(
        aspect_ratio(PixelSize {
            width: 0,
            height: 1080,
        }),
        None
    );
}

#[test]
fn aspect_ratio_formats_reduced_and_decimal_ratios() {
    assert_eq!(
        aspect_ratio(PixelSize {
            width: 1920,
            height: 1080,
        }),
        Some("16:9".to_owned())
    );
    assert_eq!(
        aspect_ratio(PixelSize {
            width: 1366,
            height: 768,
        }),
        Some("1.78:1".to_owned())
    );
}

#[test]
fn media_duration_sample_rate_and_bitrate_format_expected_units() {
    assert_eq!(duration(Duration::from_millis(400)), "0.4 s");
    assert_eq!(duration(Duration::from_secs(125)), "2:05");
    assert_eq!(duration(Duration::from_secs(3723)), "1:02:03");

    assert_eq!(sample_rate(48_000), "48 kHz");
    assert_eq!(sample_rate(44_100), "44.1 kHz");
    assert_eq!(sample_rate(22_050), "22.05 kHz");

    assert_eq!(bitrate(320_000), "320 kbit/s");
    assert_eq!(bitrate(512), "512 bit/s");
}

#[test]
fn permissions_counts_dimensions_and_line_endings_format_cleanly() {
    assert_eq!(
        dimensions(PixelSize {
            width: 3840,
            height: 2160,
        }),
        "3840 × 2160"
    );
    assert_eq!(
        permissions(&Permissions::from_mode(0o644)),
        "rw-r--r-- · 0644"
    );
    assert_eq!(count(42, false), "42");
    assert_eq!(count(10_000, true), "≥ 10000");

    assert!(matches!(
        line_endings(LineEndings::Lf),
        LineEndingLabel::Literal("LF")
    ));
    assert!(matches!(
        line_endings(LineEndings::Crlf),
        LineEndingLabel::Literal("CRLF")
    ));
    assert!(matches!(
        line_endings(LineEndings::Cr),
        LineEndingLabel::Literal("CR")
    ));
    assert!(matches!(
        line_endings(LineEndings::Mixed),
        LineEndingLabel::Message("details-mixed")
    ));
    assert_eq!(
        permissions(&Permissions::from_mode(0o4755)),
        "rwsr-xr-x · 4755"
    );
}
