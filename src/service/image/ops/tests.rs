use image::{Rgba, RgbaImage};

use super::*;

fn solid_canvas(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(width, height, Rgba(color))
}

#[test]
fn gaussian_blur_avoids_dark_transparent_halo_and_preserves_uniform_regions() {
    // Pure red opaque square surrounded by transparent black `[0, 0, 0, 0]`.
    let mut img = solid_canvas(32, 32, [0, 0, 0, 0]);
    for y in 12..20 {
        for x in 12..20 {
            img.put_pixel(x, y, Rgba([220, 40, 20, 255]));
        }
    }

    apply_gaussian_blur(&mut img, PixelRect::new(8, 8, 16, 16), 3);

    // Unblurred border outside the region must remain transparent black.
    assert_eq!(img.get_pixel(0, 0).0, [0, 0, 0, 0]);

    // Feathered boundary pixels just outside the red square gain partial alpha while keeping pure red color.
    let edge = img.get_pixel(11, 16).0;
    assert!(
        edge[3] > 0 && edge[3] < 255,
        "edge pixel should feather: {edge:?}"
    );
    assert!(
        edge[0] >= 210 && edge[1] <= 45 && edge[2] <= 25,
        "straight RGB must not darken toward [0,0,0]: {edge:?}"
    );

    // Uniform opaque region must remain exact through 3-pass separable blur.
    let mut uniform = solid_canvas(24, 24, [120, 180, 240, 255]);
    apply_gaussian_blur(&mut uniform, PixelRect::new(2, 2, 20, 20), 6);
    assert_eq!(uniform.get_pixel(12, 12).0, [120, 180, 240, 255]);
}

#[test]
fn mosaic_alpha_weights_rgb_and_solid_fill_alpha_blends() {
    let mut img = solid_canvas(4, 4, [0, 0, 0, 0]);
    img.put_pixel(0, 0, Rgba([200, 100, 50, 255]));
    apply_mosaic(&mut img, PixelRect::new(0, 0, 2, 2), 2);
    // 1 opaque [200,100,50,255] + 3 transparent [0,0,0,0] -> straight RGB remains [200,100,50], alpha ~ 64.
    let p = img.get_pixel(1, 1).0;
    assert_eq!(p[0], 200);
    assert_eq!(p[1], 100);
    assert_eq!(p[2], 50);
    assert!(p[3] > 60 && p[3] < 68);

    // Fully transparent block stays `[0, 0, 0, 0]`.
    apply_mosaic(&mut img, PixelRect::new(2, 2, 2, 2), 2);
    assert_eq!(img.get_pixel(2, 2).0, [0, 0, 0, 0]);

    // Translucent solid fill source-over blends onto opaque white; zero-alpha is a no-op.
    let mut bg = solid_canvas(4, 4, [255, 255, 255, 255]);
    apply_solid_fill(&mut bg, PixelRect::new(1, 1, 2, 2), [0, 0, 255, 0]);
    assert_eq!(bg.get_pixel(1, 1).0, [255, 255, 255, 255]);
    apply_solid_fill(&mut bg, PixelRect::new(1, 1, 2, 2), [0, 0, 255, 128]);
    let blended = bg.get_pixel(1, 1).0;
    assert!(blended[0] > 115 && blended[0] < 140);
    assert!(blended[2] == 255);
    assert_eq!(blended[3], 255);
}

#[test]
fn composite_ops_renders_dirty_bbox_strips_xml_control_chars_and_handles_edge_geometries() {
    let mut canvas = solid_canvas(240, 200, [255, 255, 255, 255]);
    let ops = vec![
        // Negative width/height normalized into [160..200, 140..180].
        RasterOp::Rect {
            x: 200.0,
            y: 180.0,
            width: -40.0,
            height: -40.0,
            color: [255, 0, 0, 255],
            stroke_width: 2.0,
            fill: Some([0, 200, 0, 255]),
        },
        // Short arrow stays bounded without backward overshoot.
        RasterOp::Arrow {
            start: [10.0, 100.0],
            end: [20.0, 100.0],
            color: [0, 0, 255, 255],
            stroke_width: 4.0,
        },
        // Degenerate zero-length and non-finite ops are safely skipped.
        RasterOp::Arrow {
            start: [50.0, 50.0],
            end: [50.0, 50.0],
            color: [0, 0, 0, 255],
            stroke_width: 4.0,
        },
        RasterOp::Line {
            start: [f32::NAN, 10.0],
            end: [20.0, 10.0],
            color: [0, 0, 0, 255],
            stroke_width: 2.0,
        },
        // XML 1.0 control characters stripped from text cleanly.
        RasterOp::Text {
            family: "sans-serif".into(),
            position: [20.0, 20.0],
            content: "OK\x00\x07\x1b!".to_owned(),
            font_size: 16.0,
            color: [10, 10, 10, 255],
        },
        // Step badge with explicit transparent text alpha leaves badge fill visible cleanly.
        RasterOp::StepBadge {
            center: [120.0, 40.0],
            number: 1,
            radius: 12.0,
            fill: [255, 0, 0, 255],
            text_color: [255, 255, 255, 0],
        },
    ];

    composite_ops(&mut canvas, &ops).expect("composite ops with edge geometries");

    // Inside normalized green filled rect [160..200, 140..180].
    assert_eq!(canvas.get_pixel(180, 160).0, [0, 200, 0, 255]);
    // Step badge center has transparent text_color[3]=0 so red fill must show through unmasked.
    assert_eq!(canvas.get_pixel(120, 40).0, [255, 0, 0, 255]);
    // Area away from all ops remains untouched white.
    assert_eq!(canvas.get_pixel(230, 10).0, [255, 255, 255, 255]);
    // Area where degenerate NaN/zero-len arrow sat remains untouched white.
    assert_eq!(canvas.get_pixel(50, 50).0, [255, 255, 255, 255]);
}

#[test]
fn apply_transform_covers_crop_rotations_flips_and_validation() {
    let mut img = solid_canvas(4, 2, [0, 0, 0, 255]);
    img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
    img.put_pixel(3, 1, Rgba([0, 255, 0, 255]));

    let rot180 = apply_transform(
        &img,
        &TransformSpec {
            rotation: OrthogonalRotation::Deg180,
            ..Default::default()
        },
    )
    .expect("rotate 180");
    assert_eq!(rot180.get_pixel(3, 1).0, [255, 0, 0, 255]);
    assert_eq!(rot180.get_pixel(0, 0).0, [0, 255, 0, 255]);

    let cropped = apply_transform(
        &img,
        &TransformSpec {
            crop: Some(PixelRect::new(0, 0, 2, 1)),
            rotation: OrthogonalRotation::Deg90,
            ..Default::default()
        },
    )
    .expect("crop + rotate 90");
    assert_eq!(cropped.dimensions(), (1, 2));

    assert!(
        apply_transform(
            &img,
            &TransformSpec {
                crop: Some(PixelRect::new(10, 10, 2, 2)),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        apply_transform(
            &img,
            &TransformSpec {
                resize: Some((0, 4)),
                ..Default::default()
            }
        )
        .is_err()
    );
}

fn textured_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let r = ((x * 7 + y * 3) % 256) as u8;
            let g = ((x * 5 + y * 11) % 256) as u8;
            let b = ((x * 13 + y * 17) % 256) as u8;
            let a = if (x + y) % 19 == 0 { 160 } else { 255 };
            img.put_pixel(x, y, Rgba([r, g, b, a]));
        }
    }
    img
}

fn point_in_rect(x: u32, y: u32, rect: PixelRect) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
}

fn assert_clipped_pixels_match(
    full: &RgbaImage,
    clipped: &RgbaImage,
    original: &RgbaImage,
    clip: PixelRect,
) {
    assert_eq!(full.dimensions(), clipped.dimensions());
    assert_eq!(original.dimensions(), clipped.dimensions());
    let (w, h) = clipped.dimensions();
    for y in 0..h {
        for x in 0..w {
            let actual = clipped.get_pixel(x, y).0;
            if point_in_rect(x, y, clip) {
                let expected = full.get_pixel(x, y).0;
                assert_eq!(
                    actual, expected,
                    "pixel ({x}, {y}) inside clip differed from unclipped composite"
                );
            } else {
                let untouched = original.get_pixel(x, y).0;
                assert_eq!(
                    actual, untouched,
                    "pixel ({x}, {y}) outside clip was unexpectedly modified"
                );
            }
        }
    }
}

fn stacked_clip_ops() -> Vec<RasterOp> {
    vec![
        // Pre-redaction vector shape strictly inside clip away from blur halo.
        RasterOp::Rect {
            x: 45.0,
            y: 35.0,
            width: 20.0,
            height: 15.0,
            color: [240, 60, 20, 255],
            stroke_width: 2.0,
            fill: Some([40, 180, 90, 220]),
        },
        // Blur region straddling top and left edges of clip (30, 20).
        RasterOp::GaussianBlur {
            region: PixelRect::new(10, 10, 70, 55),
            radius: 3,
        },
        // Mosaic anchored at (68, 36) with 8x8 blocks straddling right (110) and bottom (90) edges.
        RasterOp::Mosaic {
            region: PixelRect::new(68, 36, 56, 64),
            block_size: 8,
        },
        // Translucent solid fill straddling left and bottom clip edges.
        RasterOp::SolidFill {
            region: PixelRect::new(20, 72, 35, 30),
            color: [20, 90, 210, 128],
        },
        // Stacked vector shapes crossing clip boundaries after redactions.
        RasterOp::Rect {
            x: 65.0,
            y: 55.0,
            width: -50.0,
            height: -40.0,
            color: [255, 220, 0, 255],
            stroke_width: 2.0,
            fill: None,
        },
        RasterOp::Ellipse {
            x: 50.0,
            y: 35.0,
            width: 45.0,
            height: 35.0,
            color: [200, 30, 190, 255],
            stroke_width: 2.5,
            fill: Some([20, 140, 220, 100]),
        },
        RasterOp::Arrow {
            start: [55.0, 65.0],
            end: [128.0, 80.0],
            color: [255, 50, 50, 255],
            stroke_width: 3.0,
        },
        RasterOp::Line {
            start: [40.0, 50.0],
            end: [95.0, 105.0],
            color: [10, 220, 180, 255],
            stroke_width: 2.0,
        },
        RasterOp::Polyline {
            points: vec![[35.0, 25.0], [70.0, 85.0], [105.0, 45.0]],
            color: [250, 140, 10, 255],
            stroke_width: 2.0,
            opacity: 0.85,
        },
        RasterOp::StepBadge {
            center: [95.0, 75.0],
            number: 2,
            radius: 10.0,
            fill: [220, 20, 60, 255],
            text_color: [255, 255, 255, 255],
        },
        RasterOp::Text {
            family: "sans-serif".into(),
            position: [38.0, 28.0],
            content: "Hello\nClip".to_owned(),
            font_size: 14.0,
            color: [15, 15, 15, 255],
        },
        // Completely off-clip vector op must not leak or expand clip.
        RasterOp::Rect {
            x: 0.0,
            y: 0.0,
            width: 14.0,
            height: 10.0,
            color: [255, 0, 255, 255],
            stroke_width: 2.0,
            fill: Some([255, 0, 255, 255]),
        },
    ]
}

#[test]
fn measure_text_bounds_distinguishes_widths_multiline_cjk_and_rejects_blank() {
    assert_eq!(measure_text_bounds([10.0, 20.0], "", 16.0), None);
    assert_eq!(measure_text_bounds([10.0, 20.0], "   \n\t ", 16.0), None);
    assert_eq!(measure_text_bounds([f32::NAN, 20.0], "abc", 16.0), None);
    assert_eq!(
        measure_text_bounds([10.0, 20.0], "abc", f32::INFINITY),
        None
    );

    let narrow =
        measure_text_bounds([12.0, 24.0], "iiii", 20.0).expect("narrow text should produce bounds");
    let wide =
        measure_text_bounds([12.0, 24.0], "WWWW", 20.0).expect("wide text should produce bounds");
    assert!(
        narrow[2] < wide[2],
        "expected 'iiii' width ({}) < 'WWWW' width ({})",
        narrow[2],
        wide[2]
    );

    let single_line = measure_text_bounds([10.0, 15.0], "a", 18.0).expect("single line bounds");
    let multi_line = measure_text_bounds([10.0, 15.0], "a\nb", 18.0).expect("multiline bounds");
    assert!(
        multi_line[3] > single_line[3],
        "expected multiline height ({}) > single line height ({})",
        multi_line[3],
        single_line[3]
    );

    let cjk = measure_text_bounds([8.0, 16.0], "中文测试", 20.0).expect("CJK bounds");
    let cjk_single = measure_text_bounds([8.0, 16.0], "中", 20.0).expect("single CJK bounds");
    assert!(cjk[2] > cjk_single[2] && cjk[3] > 0.0);

    // Deterministic sandbox fallback metrics behave identically regardless of system fonts.
    let fb_narrow = fallback_text_bounds([12.0, 24.0], "iiii", 20.0);
    let fb_wide = fallback_text_bounds([12.0, 24.0], "WWWW", 20.0);
    assert!(fb_narrow[2] < fb_wide[2]);
    let fb_multi = fallback_text_bounds([10.0, 15.0], "a\nb", 20.0);
    let fb_trailing_nl = fallback_text_bounds([10.0, 15.0], "a\n", 20.0);
    assert_eq!(fb_multi[3], 2.0 * 20.0 * 1.25);
    assert_eq!(fb_trailing_nl[3], 2.0 * 20.0 * 1.25);
    let fb_cjk = fallback_text_bounds([0.0, 0.0], "中文", 20.0);
    assert_eq!(fb_cjk[2], 40.0);

    // Dirty bounds expand measure_text_bounds by 4.0 on all sides.
    let dirty = op_dirty_bounds(&RasterOp::Text {
        family: "sans-serif".into(),
        position: [12.0, 24.0],
        content: "WWWW".to_owned(),
        font_size: 20.0,
        color: [0, 0, 0, 255],
    })
    .expect("text dirty bounds");
    assert_eq!(
        dirty,
        (
            wide[0] - 4.0,
            wide[1] - 4.0,
            wide[0] + wide[2] + 4.0,
            wide[1] + wide[3] + 4.0
        )
    );
}

#[test]
fn composite_ops_clipped_matches_unclipped_inside_clip_and_preserves_outside() {
    let original = textured_canvas(160, 120);
    let ops = stacked_clip_ops();
    let clip = PixelRect::new(30, 20, 80, 70);

    let mut full = original.clone();
    composite_ops(&mut full, &ops).expect("unclipped composite");

    let mut clipped = original.clone();
    composite_ops_clipped(&mut clipped, &ops, Some(clip)).expect("clipped composite");
    assert_clipped_pixels_match(&full, &clipped, &original, clip);

    let mut unclipped_via_none = original.clone();
    composite_ops_clipped(&mut unclipped_via_none, &ops, None).expect("none clip composite");
    assert_eq!(unclipped_via_none.as_raw(), full.as_raw());

    let mut outside_clip = original.clone();
    composite_ops_clipped(
        &mut outside_clip,
        &ops,
        Some(PixelRect::new(500, 500, 20, 20)),
    )
    .expect("empty clamped clip should succeed");
    assert_eq!(outside_clip.as_raw(), original.as_raw());
}

fn verify_slice_clip(base: &RgbaImage, full: &RgbaImage, ops: &[RasterOp], clip: PixelRect) {
    let mut clipped = base.clone();
    composite_ops_clipped(&mut clipped, ops, Some(clip)).expect("slice clip composite");
    assert_clipped_pixels_match(full, &clipped, base, clip);
}

#[test]
fn composite_ops_clipped_matches_unclipped_on_sliced_curves_and_text() {
    let base = textured_canvas(200, 140);
    let ops = vec![
        RasterOp::Ellipse {
            x: 20.0,
            y: 20.0,
            width: 80.0,
            height: 60.0,
            color: [220, 40, 40, 255],
            stroke_width: 3.0,
            fill: None,
        },
        RasterOp::Line {
            start: [15.0, 95.0],
            end: [145.0, 35.0],
            color: [30, 160, 230, 255],
            stroke_width: 3.5,
        },
        RasterOp::Text {
            family: "sans-serif".into(),
            position: [50.0, 50.0],
            content: "iiii vs WWWW\n中文多行标注".to_owned(),
            font_size: 20.0,
            color: [10, 10, 10, 255],
        },
    ];
    let mut full = base.clone();
    composite_ops(&mut full, &ops).expect("full vector composite");

    // 1px vertical strip slicing diagonally across Line, Ellipse arc, and Text glyphs.
    verify_slice_clip(&base, &full, &ops, PixelRect::new(63, 20, 1, 80));
    // Sub-region slicing across curved Ellipse edge, diagonal Line, and mid-glyph Text.
    verify_slice_clip(&base, &full, &ops, PixelRect::new(60, 45, 45, 35));
}

#[test]
fn polyline_renders_single_dot_straight_segment_and_smooth_curve_with_exact_clip_parity() {
    let base = solid_canvas(120, 100, [255, 255, 255, 255]);
    let ops = vec![
        // Single-point brush tap renders a filled circular dot.
        RasterOp::Polyline {
            points: vec![[20.0, 20.0]],
            color: [239, 68, 68, 255],
            stroke_width: 8.0,
            opacity: 1.0,
        },
        // Two-point stroke renders a straight round-capped segment.
        RasterOp::Polyline {
            points: vec![[40.0, 20.0], [100.0, 20.0]],
            color: [34, 197, 94, 255],
            stroke_width: 4.0,
            opacity: 1.0,
        },
        // Multi-point freehand curve stays smooth and matches clipped rendering bit-for-bit.
        RasterOp::Polyline {
            points: vec![[15.0, 60.0], [45.0, 40.0], [75.0, 80.0], [105.0, 55.0]],
            color: [59, 130, 246, 255],
            stroke_width: 5.0,
            opacity: 1.0,
        },
    ];

    let mut full = base.clone();
    composite_ops(&mut full, &ops).expect("brush polylines composite");

    assert_eq!(full.get_pixel(20, 20).0, [239, 68, 68, 255]);
    assert_eq!(full.get_pixel(70, 20).0, [34, 197, 94, 255]);
    assert_eq!(full.get_pixel(15, 60).0, [59, 130, 246, 255]);
    assert_eq!(full.get_pixel(105, 55).0, [59, 130, 246, 255]);
    assert_eq!(full.get_pixel(5, 5).0, [255, 255, 255, 255]);

    verify_slice_clip(&base, &full, &ops, PixelRect::new(35, 35, 50, 45));
}
