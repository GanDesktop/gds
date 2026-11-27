use std::sync::Arc;

use gds::service::image::Error;
use image::{Rgba, RgbaImage};
use tokio_util::sync::CancellationToken;

use crate::{
    entities::document::{AnnotationId, AnnotationItem, AnnotationKind, RgbaColor},
    shared::geometry::Rect2,
};

use super::{
    GesturePrepareInput, GestureSession, PreparedPayload, RenderCache, TransformState,
    build_render_textures, display_tiles,
};

fn assert_payload_matches_image(payload: &PreparedPayload, expected_rgba: &RgbaImage) {
    assert_eq!(payload.tiles.len(), 1);
    let rgba = &payload.tiles[0].1;
    assert_eq!(rgba.dimensions(), expected_rgba.dimensions());
    let worst = rgba
        .as_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected_rgba.as_raw().as_chunks::<4>().0.iter())
        .enumerate()
        .filter_map(|(px_idx, (actual, expected))| {
            let diff = actual
                .iter()
                .zip(expected.iter())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            // Splitting vector primitives across prefix_rgba and incremental step introduces at
            // most +-1 LSB rounding difference between tiny_skia internal Porter-Duff and
            // f32 premultiplied blend_premultiplied_over on AA overlap pixels.
            (diff > 1).then(|| {
                let w = rgba.width() as usize;
                (px_idx % w, px_idx / w, diff, actual, expected)
            })
        })
        .next();
    assert!(worst.is_none(), "pixel mismatch (>1 LSB) at {worst:?}");
}

#[test]
fn unedited_render_reuses_base_rgba_arc_and_obeys_cancellation() {
    let mut base = RgbaImage::new(8, 8);
    base.put_pixel(2, 3, Rgba([10, 20, 30, 255]));
    let base_arc = Arc::new(base);

    let cancel = CancellationToken::new();
    let prepared = build_render_textures(
        Arc::clone(&base_arc),
        None,
        TransformState::default(),
        &[],
        &cancel,
    )
    .expect("unedited build");
    assert!(Arc::ptr_eq(&prepared.composited_rgba, &base_arc));

    let pre_cancelled = CancellationToken::new();
    pre_cancelled.cancel();
    assert!(matches!(
        build_render_textures(
            base_arc,
            None,
            TransformState::default(),
            &[],
            &pre_cancelled
        ),
        Err(Error::Cancelled)
    ));
}

#[test]
fn complete_at_checks_revision_and_svg_helper_rejects_cancelled() {
    let base_arc = Arc::new(RgbaImage::new(4, 4));
    let mut cache = RenderCache::default();
    let annotation = AnnotationItem {
        id: AnnotationId(1),
        kind: AnnotationKind::Rect {
            rect: Rect2::new(0.0, 0.0, 2.0, 2.0),
            stroke: RgbaColor::RED,
            stroke_width: 1.0,
            fill: None,
        },
    };
    let prepared = build_render_textures(
        Arc::clone(&base_arc),
        None,
        TransformState::default(),
        &[annotation],
        &CancellationToken::new(),
    )
    .expect("annotated build");
    assert!(!Arc::ptr_eq(&prepared.composited_rgba, &base_arc));

    cache.complete = Some(super::CompleteComposite {
        revision: 3,
        rgba: prepared.composited_rgba,
    });
    assert!(cache.complete_at(3).is_some());
    assert!(cache.complete_at(4).is_none());

    let cancel = CancellationToken::new();
    assert!(display_tiles(&base_arc, &cancel).is_ok());
    cancel.cancel();
    assert!(matches!(
        display_tiles(&base_arc, &cancel),
        Err(Error::Cancelled)
    ));
}

#[test]
fn gesture_session_matches_full_composite_across_stacked_effects_and_preserves_complete_isolation()
{
    let mut base = RgbaImage::from_pixel(96, 96, Rgba([240, 240, 240, 255]));
    base.put_pixel(10, 10, Rgba([10, 20, 30, 255]));
    let base_arc = Arc::new(base);
    let cancel = CancellationToken::new();

    let bottom_rect = AnnotationItem {
        id: AnnotationId(1),
        kind: AnnotationKind::Rect {
            rect: Rect2::new(10.0, 10.0, 24.0, 24.0),
            stroke: RgbaColor::RED,
            stroke_width: 4.0,
            fill: Some(RgbaColor::RED),
        },
    };
    let middle_arrow = AnnotationItem {
        id: AnnotationId(2),
        kind: AnnotationKind::Arrow {
            start: [8.0, 18.0],
            end: [52.0, 18.0],
            color: RgbaColor::BLUE,
            stroke_width: 3.0,
        },
    };
    let top_blur = AnnotationItem {
        id: AnnotationId(3),
        kind: AnnotationKind::GaussianBlur {
            region: Rect2::new(40.0, 10.0, 36.0, 36.0),
            radius: 4,
        },
    };
    let top_mosaic = AnnotationItem {
        id: AnnotationId(4),
        kind: AnnotationKind::Mosaic {
            region: Rect2::new(35.0, 50.0, 30.0, 30.0),
            block_size: 6,
        },
    };

    let initial = vec![
        bottom_rect.clone(),
        middle_arrow.clone(),
        top_blur.clone(),
        top_mosaic.clone(),
    ];
    let complete_prepared = build_render_textures(
        Arc::clone(&base_arc),
        None,
        TransformState::default(),
        &initial,
        &cancel,
    )
    .expect("initial complete build");

    let mut session = GestureSession::prepare(
        GesturePrepareInput {
            base_rgba: Arc::clone(&base_arc),
            transform: TransformState::default(),
            cached_base: Some((TransformState::default(), Arc::clone(&base_arc))),
            display: None,
            complete_rgba: Some(Arc::clone(&complete_prepared.composited_rgba)),
            initial_annotations: &initial,
            first_changed_idx: 1,
        },
        &cancel,
    )
    .expect("prepare gesture session");

    // Step 1: move middle_arrow right under top_blur (and near top_mosaic).
    let moved_arrow_1 = AnnotationItem {
        id: AnnotationId(2),
        kind: AnnotationKind::Arrow {
            start: [26.0, 24.0],
            end: [68.0, 24.0],
            color: RgbaColor::BLUE,
            stroke_width: 3.0,
        },
    };
    let frame_1_items = vec![
        bottom_rect.clone(),
        moved_arrow_1,
        top_blur.clone(),
        top_mosaic.clone(),
    ];
    let payload_1 = session.step(&frame_1_items, &cancel).expect("step 1");
    let full_1 = build_render_textures(
        Arc::clone(&base_arc),
        None,
        TransformState::default(),
        &frame_1_items,
        &cancel,
    )
    .expect("full build 1");
    assert_payload_matches_image(&payload_1, &full_1.composited_rgba);

    // Step 2: move middle_arrow down into top_mosaic so mosaic samples the moved arrow cleanly.
    let moved_arrow_2 = AnnotationItem {
        id: AnnotationId(2),
        kind: AnnotationKind::Arrow {
            start: [20.0, 62.0],
            end: [60.0, 62.0],
            color: RgbaColor::BLUE,
            stroke_width: 3.0,
        },
    };
    let frame_2_items = vec![bottom_rect, moved_arrow_2, top_blur, top_mosaic];
    let payload_2 = session.step(&frame_2_items, &cancel).expect("step 2");
    let full_2 = build_render_textures(
        Arc::clone(&base_arc),
        None,
        TransformState::default(),
        &frame_2_items,
        &cancel,
    )
    .expect("full build 2");
    assert_payload_matches_image(&payload_2, &full_2.composited_rgba);
}

#[test]
fn gesture_reuses_unaffected_display_tiles() {
    let base = Arc::new(RgbaImage::new(super::DISPLAY_TILE_EDGE + 8, 32));
    let cancel = CancellationToken::new();
    let mut session = GestureSession::prepare(
        GesturePrepareInput {
            base_rgba: base,
            transform: TransformState::default(),
            cached_base: None,
            display: None,
            complete_rgba: None,
            initial_annotations: &[],
            first_changed_idx: 0,
        },
        &cancel,
    )
    .unwrap();
    let before = session.step(&[], &cancel).unwrap();
    let annotation = AnnotationItem {
        id: AnnotationId(1),
        kind: AnnotationKind::Rect {
            rect: Rect2::new(2., 2., 10., 12.),
            stroke: RgbaColor::RED,
            stroke_width: 2.,
            fill: None,
        },
    };
    let after = session
        .step(std::slice::from_ref(&annotation), &cancel)
        .unwrap();
    assert_eq!(after.tiles.len(), 2);
    assert!(!Arc::ptr_eq(&before.tiles[0].1, &after.tiles[0].1));
    assert!(Arc::ptr_eq(&before.tiles[1].1, &after.tiles[1].1));
    let committed = AnnotationItem {
        id: AnnotationId(2),
        ..annotation
    };
    let steady = session.step(&[committed], &cancel).unwrap();
    assert!(Arc::ptr_eq(&steady.tiles[0].1, &after.tiles[0].1));
}

#[test]
fn incremental_blur_matches_export_when_growing_moving_and_shrinking() {
    let base = Arc::new(RgbaImage::from_fn(88, 64, |x, y| {
        Rgba([
            (13 * x + y) as u8,
            (29 * y) as u8,
            (x * y) as u8,
            (x * 7 + y * 17) as u8,
        ])
    }));
    let cancel = CancellationToken::new();
    let mut session = GestureSession::prepare(
        GesturePrepareInput {
            base_rgba: base.clone(),
            transform: TransformState::default(),
            cached_base: None,
            display: Some(display_tiles(&base, &cancel).expect("initial tiles")),
            complete_rgba: Some(base.clone()),
            initial_annotations: &[],
            first_changed_idx: 0,
        },
        &cancel,
    )
    .expect("blur gesture");
    let mut verify = |region, radius| {
        let items = [AnnotationItem {
            id: AnnotationId(1),
            kind: AnnotationKind::GaussianBlur { region, radius },
        }];
        let frame = session.step(&items, &cancel).expect("blur preview");
        let full = build_render_textures(
            base.clone(),
            None,
            TransformState::default(),
            &items,
            &cancel,
        )
        .expect("full composite");
        assert_eq!(frame.tiles[0].1.as_raw(), full.composited_rgba.as_raw());
    };
    verify(Rect2::new(9., 4., 13., 16.), 4);
    verify(Rect2::new(9., 4., 48., 35.), 4);
    verify(Rect2::new(-5., 0., 48., 35.), 4);
    verify(Rect2::new(0., 0., 15., 24.), 4);
    verify(Rect2::new(73., 48., 28., 40.), 4);
    verify(Rect2::new(73., 48., 28., 40.), 8);
}
