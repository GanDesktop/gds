#![cfg(feature = "image-engine")]

use std::time::{Duration, Instant};

use gds::service::image::{
    PixelRect,
    ops::{RasterOp, composite_ops, composite_ops_clipped, measure_text_bounds},
};
use image::{Rgba, RgbaImage};

fn textured_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    let raw: &mut [u8] = img.as_mut();
    for y in 0..height {
        for x in 0..width {
            let off = ((y * width + x) * 4) as usize;
            raw[off] = ((x * 7 + y * 3) % 256) as u8;
            raw[off + 1] = ((x * 5 + y * 11) % 256) as u8;
            raw[off + 2] = ((x * 13 + y * 17) % 256) as u8;
            raw[off + 3] = if (x + y) % 23 == 0 { 170 } else { 255 };
        }
    }
    img
}

fn solid_canvas(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(width, height, Rgba(color))
}

fn poison_rect(image: &mut RgbaImage, rect: PixelRect, color: [u8; 4]) {
    let Some(clamped) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };
    for y in clamped.y..clamped.bottom() {
        for x in clamped.x..clamped.right() {
            image.put_pixel(x, y, Rgba(color));
        }
    }
}

fn restore_rect(dst: &mut RgbaImage, src: &RgbaImage, rect: PixelRect) {
    let Some(clamped) = rect.clamp_to(dst.width().min(src.width()), dst.height().min(src.height()))
    else {
        return;
    };
    let dst_stride = dst.width() as usize * 4;
    let src_stride = src.width() as usize * 4;
    let x_byte = clamped.x as usize * 4;
    let row_bytes = clamped.width as usize * 4;
    let dst_raw: &mut [u8] = dst.as_mut();
    let src_raw = src.as_raw();
    for row in clamped.y as usize..clamped.bottom() as usize {
        let dst_off = row * dst_stride + x_byte;
        let src_off = row * src_stride + x_byte;
        dst_raw[dst_off..dst_off + row_bytes]
            .copy_from_slice(&src_raw[src_off..src_off + row_bytes]);
    }
}

fn point_in_rect(x: u32, y: u32, rect: PixelRect) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
}

fn check_clipped_parity(
    base: &RgbaImage,
    prefix: &RgbaImage,
    full: &RgbaImage,
    suffix_ops: &[RasterOp],
    clip: PixelRect,
    label: &str,
) -> Option<String> {
    let (w, h) = base.dimensions();
    assert_eq!(prefix.dimensions(), (w, h));
    assert_eq!(full.dimensions(), (w, h));

    let mut work = prefix.clone();
    poison_rect(&mut work, clip, [255, 0, 255, 255]);
    restore_rect(&mut work, prefix, clip);
    composite_ops_clipped(&mut work, suffix_ops, Some(clip))
        .expect("clipped composite should succeed");

    let clamped = clip.clamp_to(w, h);
    let mut inside_mismatches = 0usize;
    let mut outside_mismatches = 0usize;
    let mut max_channel_diff = 0u8;
    let mut samples: Vec<(u32, u32, [u8; 4], [u8; 4])> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let actual = work.get_pixel(x, y).0;
            let in_clip = clamped.is_some_and(|c| point_in_rect(x, y, c));
            if in_clip {
                let expected = full.get_pixel(x, y).0;
                if actual != expected {
                    inside_mismatches += 1;
                    for c in 0..4 {
                        max_channel_diff = max_channel_diff.max(actual[c].abs_diff(expected[c]));
                    }
                    if samples.len() < 5 {
                        samples.push((x, y, actual, expected));
                    }
                }
            } else {
                let untouched = prefix.get_pixel(x, y).0;
                if actual != untouched {
                    outside_mismatches += 1;
                }
            }
        }
    }
    if inside_mismatches > 0 || outside_mismatches > 0 {
        Some(format!(
            "[{label}] clip={clip:?} inside_mismatches={inside_mismatches} outside_mismatches={outside_mismatches} max_diff={max_channel_diff} samples={samples:?}"
        ))
    } else {
        None
    }
}

fn assert_clipped_parity(
    base: &RgbaImage,
    prefix: &RgbaImage,
    full: &RgbaImage,
    suffix_ops: &[RasterOp],
    clip: PixelRect,
    label: &str,
) {
    if let Some(err) = check_clipped_parity(base, prefix, full, suffix_ops, clip, label) {
        panic!("{err}");
    }
}

fn assert_clip_list_parity(
    base: &RgbaImage,
    prefix: &RgbaImage,
    full: &RgbaImage,
    suffix_ops: &[RasterOp],
    clips: &[(&str, PixelRect)],
) {
    let mut errors = Vec::new();
    for &(label, clip) in clips {
        if let Some(err) = check_clipped_parity(base, prefix, full, suffix_ops, clip, label) {
            errors.push(err);
        }
    }
    if !errors.is_empty() {
        panic!("{} clip(s) differed:\n{}", errors.len(), errors.join("\n"));
    }
}

fn rich_torture_scene_ops() -> Vec<RasterOp> {
    vec![
        // Index 0: background filled/stroked rectangle baked into prefix.
        RasterOp::Rect {
            x: 24.0,
            y: 20.0,
            width: 220.0,
            height: 150.0,
            color: [230, 60, 40, 255],
            stroke_width: 3.0,
            fill: Some([40, 170, 110, 180]),
        },
        // Index 1: long arrow crossing underneath BOTH GaussianBlur [2] and Mosaic [3] (R2).
        RasterOp::Arrow {
            start: [30.0, 108.0],
            end: [395.0, 175.0],
            color: [255, 40, 40, 255],
            stroke_width: 5.0,
        },
        // Index 2: GaussianBlur over lower rect & arrow (x: 100..210, y: 70..150, radius 4).
        RasterOp::GaussianBlur {
            region: PixelRect::new(100, 70, 110, 80),
            radius: 4,
        },
        // Index 3: unaligned Mosaic over lower arrow (origin 237,113, odd 11x11 blocks, partial edge).
        RasterOp::Mosaic {
            region: PixelRect::new(237, 113, 103, 87),
            block_size: 11,
        },
        // Index 4: translucent SolidFill disjoint from blur and mosaic (x: 52..167, y: 185..260).
        RasterOp::SolidFill {
            region: PixelRect::new(52, 185, 115, 75),
            color: [20, 100, 230, 140],
        },
        // Index 5: Ellipse overlapping SolidFill.
        RasterOp::Ellipse {
            x: 80.0,
            y: 195.0,
            width: 150.0,
            height: 95.0,
            color: [200, 30, 190, 255],
            stroke_width: 3.0,
            fill: Some([250, 200, 40, 90]),
        },
        // Index 6: Line crossing lower canvas.
        RasterOp::Line {
            start: [45.0, 275.0],
            end: [320.0, 315.0],
            color: [10, 210, 175, 255],
            stroke_width: 3.5,
        },
        // Index 7: Arrow with prominent arrowhead.
        RasterOp::Arrow {
            start: [195.0, 270.0],
            end: [345.0, 235.0],
            color: [30, 80, 240, 255],
            stroke_width: 4.5,
        },
        // Index 8: Negative-dimension hollow Rect.
        RasterOp::Rect {
            x: 215.0,
            y: 325.0,
            width: -70.0,
            height: -45.0,
            color: [10, 150, 40, 255],
            stroke_width: 2.5,
            fill: None,
        },
        // Index 9: Mixed Latin/CJK multiline text placed clear of Ellipse [5] for isolated slicing checks.
        RasterOp::Text {
            family: "sans-serif".into(),
            position: [260.0, 245.0],
            content: "iiii vs WWWW\n中文多行标注".to_owned(),
            font_size: 22.0,
            color: [18, 22, 28, 255],
        },
        // Index 10: Off-clip edge Rect.
        RasterOp::Rect {
            x: 420.0,
            y: 10.0,
            width: 40.0,
            height: 30.0,
            color: [255, 0, 128, 255],
            stroke_width: 2.0,
            fill: Some([255, 0, 128, 200]),
        },
    ]
}

fn r2_dragged_arrow_scene(arrow_start: [f32; 2], arrow_end: [f32; 2]) -> Vec<RasterOp> {
    vec![
        RasterOp::Rect {
            x: 24.0,
            y: 20.0,
            width: 110.0,
            height: 55.0,
            color: [230, 60, 40, 255],
            stroke_width: 3.0,
            fill: Some([40, 170, 110, 180]),
        },
        RasterOp::Arrow {
            start: arrow_start,
            end: arrow_end,
            color: [255, 40, 40, 255],
            stroke_width: 5.0,
        },
        RasterOp::GaussianBlur {
            region: PixelRect::new(100, 70, 110, 80),
            radius: 4,
        },
        RasterOp::Mosaic {
            region: PixelRect::new(237, 113, 103, 87),
            block_size: 11,
        },
        RasterOp::Ellipse {
            x: 150.0,
            y: 95.0,
            width: 120.0,
            height: 75.0,
            color: [20, 160, 220, 255],
            stroke_width: 2.5,
            fill: None,
        },
        RasterOp::Text {
            family: "sans-serif".into(),
            position: [130.0, 160.0],
            content: "iiii vs WWWW\n中文多行标注".to_owned(),
            font_size: 20.0,
            color: [15, 20, 25, 255],
        },
    ]
}

fn non_background_bbox(img: &RgbaImage, bg: [u8; 4]) -> Option<[f32; 4]> {
    let (w, h) = img.dimensions();
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for y in 0..h {
        for x in 0..w {
            if img.get_pixel(x, y).0 != bg {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
    }
    (min_x < max_x && min_y < max_y).then_some([
        min_x as f32,
        min_y as f32,
        (max_x - min_x) as f32,
        (max_y - min_y) as f32,
    ])
}

fn assert_text_alignment_with_rendered_pixels(content: &str, position: [f32; 2], font_size: f32) {
    let bounds = measure_text_bounds(position, content, font_size)
        .expect("non-empty text must produce bounds");
    assert!(
        bounds.iter().all(|v| v.is_finite()) && bounds[2] > 0.0 && bounds[3] > 0.0,
        "text bounds must be finite and positive for {content:?}: {bounds:?}"
    );

    let bg = [255, 255, 255, 255];
    let mut canvas = solid_canvas(640, 240, bg);
    composite_ops(
        &mut canvas,
        &[RasterOp::Text {
            family: "sans-serif".into(),
            position,
            content: content.to_owned(),
            font_size,
            color: [10, 10, 10, 255],
        }],
    )
    .expect("composite text should succeed");

    let Some(rendered) = non_background_bbox(&canvas, bg) else {
        // Headless environment with zero fontdb glyphs available for this script.
        return;
    };

    let tol = 2.0;
    assert!(
        rendered[0] >= bounds[0] - tol && rendered[1] >= bounds[1] - tol,
        "rendered pixels start ({}, {}) exceeded measured top-left ({}, {}) for {content:?}",
        rendered[0],
        rendered[1],
        bounds[0],
        bounds[1]
    );
    assert!(
        rendered[0] + rendered[2] <= bounds[0] + bounds[2] + tol
            && rendered[1] + rendered[3] <= bounds[1] + bounds[3] + tol,
        "rendered pixels end ({}, {}) exceeded measured bottom-right ({}, {}) for {content:?}",
        rendered[0] + rendered[2],
        rendered[1] + rendered[3],
        bounds[0] + bounds[2],
        bounds[1] + bounds[3]
    );
    assert!(
        (bounds[2] - rendered[2]).abs() <= 4.0
            && (bounds[3] - rendered[3]).abs() <= font_size * 0.55,
        "measured bounds {bounds:?} should tightly fit rendered pixel bbox {rendered:?} for {content:?}"
    );
}

fn section_8_5_benchmark_ops() -> Vec<RasterOp> {
    let ops = vec![
        // 0..10: Prefix scene
        RasterOp::Rect {
            x: 80.0,
            y: 70.0,
            width: 420.0,
            height: 260.0,
            color: [220, 50, 40, 255],
            stroke_width: 3.0,
            fill: None,
        },
        RasterOp::Arrow {
            start: [140.0, 400.0],
            end: [520.0, 290.0],
            color: [240, 60, 30, 255],
            stroke_width: 4.0,
        },
        RasterOp::Rect {
            x: 620.0,
            y: 120.0,
            width: 360.0,
            height: 240.0,
            color: [30, 140, 230, 255],
            stroke_width: 3.0,
            fill: Some([30, 140, 230, 40]),
        },
        RasterOp::Ellipse {
            x: 1120.0,
            y: 140.0,
            width: 280.0,
            height: 180.0,
            color: [180, 40, 200, 255],
            stroke_width: 3.0,
            fill: None,
        },
        RasterOp::Arrow {
            start: [1040.0, 360.0],
            end: [1420.0, 220.0],
            color: [20, 170, 90, 255],
            stroke_width: 4.0,
        },
        RasterOp::Rect {
            x: 1540.0,
            y: 100.0,
            width: 460.0,
            height: 280.0,
            color: [230, 150, 20, 255],
            stroke_width: 3.0,
            fill: None,
        },
        RasterOp::Line {
            start: [120.0, 760.0],
            end: [760.0, 780.0],
            color: [10, 190, 190, 255],
            stroke_width: 3.0,
        },
        RasterOp::Text {
            family: "sans-serif".into(),
            position: [110.0, 105.0],
            content: "Step 1: 登录校验 Auth".to_owned(),
            font_size: 24.0,
            color: [20, 25, 30, 255],
        },
        RasterOp::Rect {
            x: 180.0,
            y: 860.0,
            width: 440.0,
            height: 300.0,
            color: [200, 40, 120, 255],
            stroke_width: 3.0,
            fill: Some([200, 40, 120, 30]),
        },
        RasterOp::Arrow {
            start: [560.0, 960.0],
            end: [860.0, 840.0],
            color: [240, 80, 20, 255],
            stroke_width: 4.0,
        },
        // 10..16: Middle ops including Text #2, Ellipse #2, Text #3, Arrow crossing blur #1, Rect, GaussianBlur #1
        RasterOp::Ellipse {
            x: 1320.0,
            y: 520.0,
            width: 260.0,
            height: 190.0,
            color: [50, 100, 240, 255],
            stroke_width: 3.0,
            fill: None,
        },
        RasterOp::Text {
            family: "sans-serif".into(),
            position: [650.0, 150.0],
            content: "iiii vs WWWW\n中文多行标注".to_owned(),
            font_size: 22.0,
            color: [18, 18, 18, 255],
        },
        RasterOp::Rect {
            x: 1920.0,
            y: 440.0,
            width: 480.0,
            height: 320.0,
            color: [80, 180, 60, 255],
            stroke_width: 3.0,
            fill: None,
        },
        // Index 13: Arrow crossing under GaussianBlur #1 (index 15) inside (870, 470, 320, 220).
        RasterOp::Arrow {
            start: [885.0, 515.0],
            end: [1155.0, 640.0],
            color: [255, 45, 45, 255],
            stroke_width: 4.0,
        },
        RasterOp::Text {
            family: "sans-serif".into(),
            position: [1580.0, 130.0],
            content: "Export 导出 2560×1440".to_owned(),
            font_size: 24.0,
            color: [24, 24, 24, 255],
        },
        // Index 15: GaussianBlur #1 (intersected by 320x220 dirty region (870, 470, 320, 220)).
        RasterOp::GaussianBlur {
            region: PixelRect::new(920, 480, 250, 180),
            radius: 8,
        },
        // 16..20: Top ops (dragged rect & arrow inside 320x220 non-blur dirty region (160, 480, 320, 220)).
        RasterOp::Rect {
            x: 185.0,
            y: 505.0,
            width: 240.0,
            height: 150.0,
            color: [230, 55, 55, 255],
            stroke_width: 3.0,
            fill: None,
        },
        RasterOp::Arrow {
            start: [205.0, 525.0],
            end: [445.0, 665.0],
            color: [25, 110, 245, 255],
            stroke_width: 4.0,
        },
        // Index 18: GaussianBlur #2.
        RasterOp::GaussianBlur {
            region: PixelRect::new(1680, 860, 260, 180),
            radius: 6,
        },
        // Index 19: Mixed CJK/Latin Text #4.
        RasterOp::Text {
            family: "sans-serif".into(),
            position: [1480.0, 1140.0],
            content: "边界对齐 Boundary Check".to_owned(),
            font_size: 26.0,
            color: [15, 20, 30, 255],
        },
    ];
    assert_eq!(ops.len(), 20, "Section 8.5 workload must contain 20 ops");
    ops
}

fn measure_mean(iterations: usize, mut body: impl FnMut()) -> Duration {
    let mut total = Duration::ZERO;
    for _ in 0..iterations {
        let start = Instant::now();
        body();
        total += start.elapsed();
    }
    total / iterations as u32
}

#[test]
fn clipped_vs_full_pixel_parity_torture_across_redactions_and_edges() {
    let base = textured_canvas(480, 360);
    let ops = rich_torture_scene_ops();

    let mut full = base.clone();
    composite_ops(&mut full, &ops).expect("full composite should succeed");

    let mut unclipped_none = base.clone();
    composite_ops_clipped(&mut unclipped_none, &ops, None).expect("None clip should succeed");
    assert_eq!(unclipped_none.as_raw(), full.as_raw());

    let mut prefix = base.clone();
    composite_ops(&mut prefix, &ops[..2]).expect("prefix composite should succeed");
    let suffix = &ops[2..];

    let clips = [
        ("mosaic_unaligned_blocks", PixelRect::new(242, 118, 96, 80)),
        ("mosaic_interior_seam", PixelRect::new(253, 130, 19, 17)),
        ("blur_top_left_halo", PixelRect::new(94, 65, 30, 40)),
        ("blur_bottom_right_halo", PixelRect::new(105, 80, 112, 76)),
        (
            "vectors_ellipse_arrowhead_and_text",
            PixelRect::new(68, 188, 355, 145),
        ),
        ("ellipse_top_arc_slice", PixelRect::new(72, 220, 165, 65)),
        ("1px_wide_ellipse_strip", PixelRect::new(112, 190, 1, 110)),
        ("text_between_lines", PixelRect::new(250, 271, 180, 45)),
        (
            "mid_glyph_text_baseline_slice",
            PixelRect::new(265, 258, 130, 32),
        ),
        ("1px_wide_text_strip", PixelRect::new(295, 240, 1, 70)),
        ("arrowhead_only", PixelRect::new(332, 224, 24, 24)),
        ("1px_mosaic_cell", PixelRect::new(250, 132, 1, 1)),
        ("1px_blur_interior", PixelRect::new(150, 100, 1, 1)),
        ("1px_wide_rect_line_strip", PixelRect::new(190, 270, 1, 60)),
        ("partially_off_canvas", PixelRect::new(320, 220, 300, 300)),
        ("completely_off_canvas", PixelRect::new(500, 400, 40, 40)),
        ("zero_width_clip", PixelRect::new(100, 100, 0, 20)),
    ];
    assert_clip_list_parity(&base, &prefix, &full, suffix, &clips);
}

#[test]
fn clipped_parity_r2_dragged_arrow_under_blur_and_mosaic() {
    let base = textured_canvas(480, 360);
    let prev_ops = r2_dragged_arrow_scene([35.0, 95.0], [360.0, 155.0]);
    let curr_ops = r2_dragged_arrow_scene([45.0, 120.0], [355.0, 185.0]);

    let mut prefix = base.clone();
    composite_ops(&mut prefix, &curr_ops[..1]).expect("prefix composite");

    let mut work = base.clone();
    composite_ops(&mut work, &prev_ops).expect("previous frame composite");

    let mut full_curr = base.clone();
    composite_ops(&mut full_curr, &curr_ops).expect("full current frame composite");

    // Union of previous arrow, new arrow, intersecting blur (+12px halo), and intersecting mosaic.
    let dirty_rect = PixelRect::new(20, 56, 354, 170);
    restore_rect(&mut work, &prefix, dirty_rect);
    composite_ops_clipped(&mut work, &curr_ops[1..], Some(dirty_rect))
        .expect("clipped R2 drag step should succeed");

    assert_images_byte_identical(
        &work,
        &full_curr,
        "R2 dragged arrow crossing under GaussianBlur and Mosaic",
    );
}

fn assert_images_byte_identical(actual: &RgbaImage, expected: &RgbaImage, label: &str) {
    assert_eq!(actual.dimensions(), expected.dimensions());
    let (w, h) = actual.dimensions();
    let mut count = 0usize;
    let mut samples: Vec<(u32, u32, [u8; 4], [u8; 4])> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let a = actual.get_pixel(x, y).0;
            let e = expected.get_pixel(x, y).0;
            if a != e {
                count += 1;
                if samples.len() < 12 {
                    samples.push((x, y, a, e));
                }
            }
        }
    }
    if !samples.is_empty() {
        panic!("[{label}] {count} pixel(s) differed; samples={samples:?}");
    }
}

#[test]
fn measure_text_bounds_proportional_multiline_cjk_and_pixel_alignment() {
    assert_eq!(measure_text_bounds([10.0, 20.0], "", 20.0), None);
    assert_eq!(measure_text_bounds([10.0, 20.0], "   \n\t ", 20.0), None);
    assert_eq!(measure_text_bounds([f32::NAN, 20.0], "abc", 20.0), None);
    assert_eq!(
        measure_text_bounds([10.0, 20.0], "abc", f32::INFINITY),
        None
    );

    let narrow = measure_text_bounds([20.0, 30.0], "iiii", 24.0).expect("narrow text bounds");
    let wide = measure_text_bounds([20.0, 30.0], "WWWW", 24.0).expect("wide text bounds");
    assert!(
        narrow[2] < wide[2],
        "expected proportional 'iiii' width ({}) < 'WWWW' width ({})",
        narrow[2],
        wide[2]
    );

    let single = measure_text_bounds([20.0, 30.0], "a", 20.0).expect("single line");
    let trailing_nl = measure_text_bounds([20.0, 30.0], "a\n", 20.0).expect("trailing newline");
    let two_lines = measure_text_bounds([20.0, 30.0], "a\nb", 20.0).expect("two lines");
    let blank_mid = measure_text_bounds([20.0, 30.0], "a\n\nb", 20.0).expect("blank middle line");
    assert!(
        trailing_nl[2] > 0.0 && trailing_nl[3] >= single[3],
        "trailing newline bounds should stay positive and >= single line: {trailing_nl:?} vs {single:?}"
    );
    assert!(
        two_lines[3] > single[3] && blank_mid[3] > two_lines[3],
        "multiline height must scale monotonically: single={}, two={}, blank_mid={}",
        single[3],
        two_lines[3],
        blank_mid[3]
    );

    let mixed_origin = measure_text_bounds([0.0, 0.0], "iiii vs WWWW\n中文多行标注", 24.0)
        .expect("mixed bounds at origin");
    let mixed_shifted = measure_text_bounds([45.0, 60.0], "iiii vs WWWW\n中文多行标注", 24.0)
        .expect("mixed bounds shifted");
    assert_eq!(mixed_shifted[0], mixed_origin[0] + 45.0);
    assert_eq!(mixed_shifted[1], mixed_origin[1] + 60.0);
    assert_eq!(mixed_shifted[2], mixed_origin[2]);
    assert_eq!(mixed_shifted[3], mixed_origin[3]);

    assert_text_alignment_with_rendered_pixels("iiii", [30.0, 40.0], 24.0);
    assert_text_alignment_with_rendered_pixels("WWWW", [30.0, 40.0], 24.0);
    assert_text_alignment_with_rendered_pixels("a\n\nb", [30.0, 40.0], 24.0);
    assert_text_alignment_with_rendered_pixels("iiii vs WWWW\n中文多行标注", [30.0, 40.0], 24.0);

    let clamped_small =
        measure_text_bounds([12.0, 18.0], "abc", 2.0).expect("small font size clamped to 8.0");
    let min_font =
        measure_text_bounds([12.0, 18.0], "abc", 8.0).expect("minimum 8.0 font size bounds");
    assert_eq!(clamped_small, min_font);
}

#[test]
fn section_8_5_benchmark_2560x1440_workload() {
    let base = textured_canvas(2560, 1440);
    let ops = section_8_5_benchmark_ops();

    // Warm up once so fontdb initialization does not skew steady-state timings.
    let mut warmup_prefix = base.clone();
    composite_ops(&mut warmup_prefix, &ops[..16]).expect("warmup prefix should succeed");

    let prefix_time = measure_mean(3, || {
        let mut prefix_canvas = base.clone();
        composite_ops(&mut prefix_canvas, &ops[..16]).expect("prefix composite should succeed");
    });

    let dirty_vector = PixelRect::new(160, 480, 320, 220);
    let mut work_vector = warmup_prefix.clone();
    let drag_vector_time = measure_mean(5, || {
        restore_rect(&mut work_vector, &warmup_prefix, dirty_vector);
        composite_ops_clipped(&mut work_vector, &ops[16..], Some(dirty_vector))
            .expect("clipped vector drag step should succeed");
    });

    let mut blur_prefix = base.clone();
    composite_ops(&mut blur_prefix, &ops[..13]).expect("blur prefix should succeed");
    let dirty_blur = PixelRect::new(870, 470, 320, 220);
    let mut work_blur = blur_prefix.clone();
    let drag_blur_time = measure_mean(5, || {
        restore_rect(&mut work_blur, &blur_prefix, dirty_blur);
        composite_ops_clipped(&mut work_blur, &ops[13..], Some(dirty_blur))
            .expect("clipped blur-intersecting drag step should succeed");
    });

    let mut full_commit = base.clone();
    let full_commit_time = measure_mean(3, || {
        full_commit.clone_from(&base);
        composite_ops(&mut full_commit, &ops).expect("full 2560x1440 commit should succeed");
    });

    let mut reference_full = base.clone();
    composite_ops(&mut reference_full, &ops).expect("reference full composite");
    assert_eq!(full_commit.as_raw(), reference_full.as_raw());
    assert_clipped_parity(
        &base,
        &warmup_prefix,
        &reference_full,
        &ops[16..],
        dirty_vector,
        "section_8_5_vector_320x220",
    );
    assert_clipped_parity(
        &base,
        &blur_prefix,
        &reference_full,
        &ops[13..],
        dirty_blur,
        "section_8_5_blur_320x220",
    );

    print_section_8_5_report(
        prefix_time,
        drag_vector_time,
        drag_blur_time,
        full_commit_time,
    );
}

fn print_section_8_5_report(
    prefix_time: Duration,
    drag_vector_time: Duration,
    drag_blur_time: Duration,
    full_commit_time: Duration,
) {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("=== Section 8.5 Benchmark (2560x1440 sRGB RGBA8, 20 ops, profile={profile}) ===");
    println!(
        "  a) Prefix preparation (0..16, clone + composite_ops): {:.2} ms",
        prefix_time.as_secs_f64() * 1000.0
    );
    println!(
        "  b1) Dirty-rect drag step (320x220 vector/rect restore + composite_ops_clipped): {:.2} ms",
        drag_vector_time.as_secs_f64() * 1000.0
    );
    println!(
        "  b2) Dirty-rect drag step (320x220 intersecting GaussianBlur r=8 + arrow): {:.2} ms",
        drag_blur_time.as_secs_f64() * 1000.0
    );
    println!(
        "  c) Full 2560x1440 composite_ops commit (all 20 ops): {:.2} ms",
        full_commit_time.as_secs_f64() * 1000.0
    );
}
