use super::aspect_ratio;
use gds::service::file_inspection::PixelSize;

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
