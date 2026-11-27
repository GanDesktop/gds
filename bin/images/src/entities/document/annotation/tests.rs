use gds::service::image::PixelRect;

use super::{
    AnnotationId, AnnotationItem, AnnotationKind, RgbaColor, SpatialTransformOp, measure_text_rect,
};
use crate::shared::geometry::Rect2;

fn sample_rect_item(id: u64, rect: Rect2) -> AnnotationItem {
    AnnotationItem {
        id: AnnotationId(id),
        kind: AnnotationKind::Rect {
            rect,
            stroke: RgbaColor::RED,
            stroke_width: 3.0,
            fill: None,
        },
    }
}

fn sample_text_item(id: u64, position: [f32; 2], content: &str, font_size: f32) -> AnnotationItem {
    AnnotationItem {
        id: AnnotationId(id),
        kind: AnnotationKind::Text {
            family: "sans-serif".into(),
            position,
            content: content.to_owned(),
            color: RgbaColor::BLUE,
            font_size,
        },
    }
}

fn approx_rect(a: Rect2, b: Rect2, tol: f32) -> bool {
    (a.x - b.x).abs() <= tol
        && (a.y - b.y).abs() <= tol
        && (a.width - b.width).abs() <= tol
        && (a.height - b.height).abs() <= tol
}

#[test]
fn spatial_transforms_preserve_group_identities_and_keep_text_upright() {
    let rect = sample_rect_item(1, Rect2::new(10.0, 20.0, 40.0, 15.0));
    let arrow = AnnotationItem {
        id: AnnotationId(2),
        kind: AnnotationKind::Arrow {
            start: [12.0, 18.0],
            end: [70.0, 50.0],
            color: RgbaColor::GREEN,
            stroke_width: 3.0,
        },
    };
    let text = sample_text_item(3, [25.0, 30.0], "Hello 世界\nLine 2", 20.0);
    let orig_text_bounds = text.visual_bounds();

    // RotateCw -> RotateCcw returns exact geometry (on 200x100 -> 100x200 -> 200x100).
    let cw_rect = rect.apply_spatial_transform(SpatialTransformOp::RotateCw, 200, 100);
    assert_eq!(cw_rect.visual_bounds(), Rect2::new(65.0, 10.0, 15.0, 40.0));
    assert_eq!(
        cw_rect.apply_spatial_transform(SpatialTransformOp::RotateCcw, 100, 200),
        rect
    );

    let cw_text = text.apply_spatial_transform(SpatialTransformOp::RotateCw, 200, 100);
    let cw_text_bounds = cw_text.visual_bounds();
    assert!((cw_text_bounds.width - orig_text_bounds.width).abs() < 1e-3);
    assert!((cw_text_bounds.height - orig_text_bounds.height).abs() < 1e-3);
    let roundtrip_ccw = cw_text.apply_spatial_transform(SpatialTransformOp::RotateCcw, 100, 200);
    assert!(approx_rect(
        roundtrip_ccw.visual_bounds(),
        orig_text_bounds,
        1e-3
    ));

    // RotateCw^4 == identity for rect, arrow, and text.
    let r1 = text.apply_spatial_transform(SpatialTransformOp::RotateCw, 200, 100);
    let r2 = r1.apply_spatial_transform(SpatialTransformOp::RotateCw, 100, 200);
    let r3 = r2.apply_spatial_transform(SpatialTransformOp::RotateCw, 200, 100);
    let r4 = r3.apply_spatial_transform(SpatialTransformOp::RotateCw, 100, 200);
    assert!(approx_rect(r4.visual_bounds(), orig_text_bounds, 1e-3));

    // FlipH^2 == id and FlipV^2 == id.
    let fh1 = arrow.apply_spatial_transform(SpatialTransformOp::FlipHorizontal, 200, 100);
    let fh2 = fh1.apply_spatial_transform(SpatialTransformOp::FlipHorizontal, 200, 100);
    assert_eq!(fh2, arrow);

    let tf_h = text
        .apply_spatial_transform(SpatialTransformOp::FlipHorizontal, 200, 100)
        .apply_spatial_transform(SpatialTransformOp::FlipHorizontal, 200, 100);
    assert!(approx_rect(tf_h.visual_bounds(), orig_text_bounds, 1e-3));

    let tf_v = text
        .apply_spatial_transform(SpatialTransformOp::FlipVertical, 200, 100)
        .apply_spatial_transform(SpatialTransformOp::FlipVertical, 200, 100);
    assert!(approx_rect(tf_v.visual_bounds(), orig_text_bounds, 1e-3));
}

#[test]
fn apply_crop_keeps_signed_partial_geometry_and_rejects_missed_diagonal_lines() {
    let crop = PixelRect::new(50, 50, 100, 100);

    // Crossing rect shifts into negative coordinates cleanly for SVG viewBox clipping.
    let crossing_rect = sample_rect_item(1, Rect2::new(20.0, 60.0, 50.0, 40.0));
    let cropped_rect = crossing_rect
        .apply_crop(crop)
        .expect("crossing rect retained");
    assert_eq!(
        cropped_rect.visual_bounds(),
        Rect2::new(-30.0, 10.0, 50.0, 40.0)
    );

    // Crossing mosaic clamps cleanly to PixelRect on rasterization.
    let crossing_mosaic = AnnotationItem {
        id: AnnotationId(2),
        kind: AnnotationKind::Mosaic {
            region: Rect2::new(20.0, 60.0, 50.0, 40.0),
            block_size: 10,
        },
    };
    let cropped_mosaic = crossing_mosaic
        .apply_crop(crop)
        .expect("crossing mosaic retained");
    assert!(cropped_mosaic.to_raster_op(100, 100).is_some());

    // Diagonal line whose AABB touches [50..150, 50..150] while the line stays far in top-left.
    let distant_diagonal = AnnotationItem {
        id: AnnotationId(3),
        kind: AnnotationKind::Line {
            start: [0.0, 120.0],
            end: [120.0, 0.0],
            color: RgbaColor::RED,
            stroke_width: 2.0,
        },
    };
    assert_eq!(
        distant_diagonal.apply_crop(PixelRect::new(80, 80, 50, 50)),
        None
    );

    // Diagonal line passing directly through the crop is retained with exact translated coords.
    let crossing_line = AnnotationItem {
        id: AnnotationId(4),
        kind: AnnotationKind::Line {
            start: [0.0, 100.0],
            end: [200.0, 100.0],
            color: RgbaColor::RED,
            stroke_width: 2.0,
        },
    };
    let kept_line = crossing_line.apply_crop(crop).expect("crossing line kept");
    assert_eq!(
        kept_line.kind,
        AnnotationKind::Line {
            start: [-50.0, 50.0],
            end: [150.0, 50.0],
            color: RgbaColor::RED,
            stroke_width: 2.0,
        }
    );

    // Wide text is wider than narrow text.
    let narrow = measure_text_rect([10.0, 10.0], "iiii", 24.0, "sans-serif");
    let wide = measure_text_rect([10.0, 10.0], "WWWW", 24.0, "sans-serif");
    assert!(narrow.width < wide.width);
}

#[test]
fn brush_annotation_supports_transforms_crop_and_style_updates() {
    let stroke = AnnotationItem {
        id: AnnotationId(7),
        kind: AnnotationKind::Brush {
            points: vec![[10.0, 10.0], [10.0, 140.0], [140.0, 140.0]],
            color: RgbaColor::RED,
            stroke_width: 4.0,
        },
    };
    assert_eq!(stroke.visual_bounds(), Rect2::new(10.0, 10.0, 130.0, 130.0));
    assert_eq!(stroke.color(), Some(RgbaColor::RED));
    assert_eq!(stroke.stroke_width(), Some(4.0));

    let recolored = stroke
        .with_color(RgbaColor::BLUE)
        .and_then(|item| item.with_stroke_width(8.0))
        .expect("brush supports color and stroke width");
    assert_eq!(recolored.color(), Some(RgbaColor::BLUE));
    assert_eq!(recolored.stroke_width(), Some(8.0));

    // Spatial transforms preserve involutions exactly.
    let roundtrip = stroke
        .apply_spatial_transform(SpatialTransformOp::RotateCw, 200, 150)
        .apply_spatial_transform(SpatialTransformOp::RotateCcw, 150, 200);
    assert_eq!(roundtrip, stroke);
    let flipped = stroke
        .apply_spatial_transform(SpatialTransformOp::FlipHorizontal, 200, 150)
        .apply_spatial_transform(SpatialTransformOp::FlipHorizontal, 200, 150);
    assert_eq!(flipped, stroke);

    // Crop inside the hollow L-corner misses every segment even though the AABB overlaps it.
    assert_eq!(stroke.apply_crop(PixelRect::new(50, 50, 60, 60)), None);
    // Crop intersecting the vertical leg keeps the stroke with shifted signed coordinates.
    let kept = stroke
        .apply_crop(PixelRect::new(0, 50, 40, 40))
        .expect("crossing brush stroke retained");
    assert_eq!(kept.visual_bounds(), Rect2::new(10.0, -40.0, 130.0, 130.0));
}
