use super::*;

#[test]
fn triangle_uses_the_same_geometry_to_pick_and_display_colors() {
    let color = RgbaColor::rgb(106, 18, 212);
    let hsv = Hsv::from_rgb(color, 0.0);
    assert_eq!(hsv.pick(Part::Triangle, hsv.marker()).rgb(), color);
    let [hue, white, black] = hsv.axes();
    assert_eq!(
        hsv.pick(Part::Triangle, white * 2.0).rgb(),
        RgbaColor::WHITE
    );
    assert_eq!(
        hsv.pick(Part::Triangle, black * 2.0).rgb(),
        RgbaColor::rgb(0, 0, 0)
    );
    let gray = Hsv::from_rgb(RgbaColor::rgb(127, 127, 127), hsv.hue);
    assert_eq!(gray.hue, hsv.hue);
    let saturated = gray.pick(Part::Triangle, hue * TRIANGLE);
    assert!((saturated.value - 1.0).abs() < 1e-6);
    assert!((saturated.saturation - 1.0).abs() < 1e-6);
}

#[test]
fn parse_hex_rgb_accepts_six_digit_hex_and_rejects_malformed_inputs() {
    assert_eq!(
        parse_hex_rgb("#FF00AA"),
        Some(RgbaColor::rgb(0xFF, 0x00, 0xAA))
    );
    assert_eq!(
        parse_hex_rgb("ff00aa"),
        Some(RgbaColor::rgb(0xFF, 0x00, 0xAA))
    );
    assert_eq!(
        parse_hex_rgb("  #12aB34  "),
        Some(RgbaColor::rgb(0x12, 0xAB, 0x34))
    );
    assert_eq!(parse_hex_rgb("000000"), Some(RgbaColor::rgb(0, 0, 0)));

    assert_eq!(parse_hex_rgb("##FF0000"), None);
    assert_eq!(parse_hex_rgb("##12345"), None);
    assert_eq!(parse_hex_rgb("+12345"), None);
    assert_eq!(parse_hex_rgb("#+12345"), None);
    assert_eq!(parse_hex_rgb("-12345"), None);
    assert_eq!(parse_hex_rgb(""), None);
    assert_eq!(parse_hex_rgb("   "), None);
    assert_eq!(parse_hex_rgb("#"), None);
    assert_eq!(parse_hex_rgb("#FFF"), None);
    assert_eq!(parse_hex_rgb("#FF0000FF"), None);
    assert_eq!(parse_hex_rgb("#GGGGGG"), None);
    assert_eq!(parse_hex_rgb("ééé"), None);
}
