use std::sync::Arc;

use image::{Rgba, RgbaImage};
use tokio_util::sync::CancellationToken;

use crate::{
    entities::document::{
        AnnotationId, AnnotationItem, AnnotationKind, RgbaColor, TransformState,
        build_render_textures,
    },
    shared::geometry::Rect2,
};

use super::{
    FrameInstallCheck, FrameInstallDecision, Request, decide_frame_install, quantize_svg_scale,
    svg_scale_matches,
};

fn patterned_image(width: u32, height: u32) -> Arc<RgbaImage> {
    Arc::new(RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            ((x * 13 + y * 5) % 256) as u8,
            ((x * 7 + y * 19) % 256) as u8,
            ((x * 23 + y * 11) % 256) as u8,
            255,
        ])
    }))
}

#[test]
fn committing_overlapping_vector_annotation_restores_exact_pixels() {
    let source = patterned_image(128, 96);
    let cancel = CancellationToken::new();

    let first_rect = AnnotationItem {
        id: AnnotationId(1),
        kind: AnnotationKind::Rect {
            rect: Rect2::new(12.0, 12.0, 56.0, 48.0),
            stroke: RgbaColor::RED,
            stroke_width: 4.0,
            fill: Some(RgbaColor {
                r: 239,
                g: 68,
                b: 68,
                a: 140,
            }),
        },
    };
    let initial = Request {
        source: source.clone(),
        transform: TransformState::default(),
        committed: Arc::from([first_rect.clone()]),
        transient: None,
        svg: None,
    };
    let (compositor, _) = initial.compose(None, &cancel).expect("initial composite");
    assert!(compositor.exact);

    let overlapping_arrow = AnnotationItem {
        id: AnnotationId(2),
        kind: AnnotationKind::Arrow {
            start: [20.0, 18.0],
            end: [96.0, 72.0],
            color: RgbaColor::BLUE,
            stroke_width: 4.5,
        },
    };
    let preview_items: Arc<[AnnotationItem]> =
        Arc::from([first_rect.clone(), overlapping_arrow.clone()]);
    let drag_preview = Request {
        source: source.clone(),
        transform: TransformState::default(),
        committed: Arc::from([first_rect.clone()]),
        transient: Some((1, preview_items.clone())),
        svg: None,
    };
    let (compositor, _) = drag_preview
        .compose(Some(compositor), &cancel)
        .expect("transient preview");

    let commit = Request {
        source: source.clone(),
        transform: TransformState::default(),
        committed: preview_items.clone(),
        transient: None,
        svg: None,
    };
    let (compositor, _) = commit
        .compose(Some(compositor), &cancel)
        .expect("committed composite");
    assert!(
        compositor.exact,
        "committed composite must remain exact so RenderCache.complete stays populated"
    );

    let reference = build_render_textures(
        source,
        None,
        TransformState::default(),
        &preview_items,
        &cancel,
    )
    .expect("reference full build");
    assert_eq!(
        compositor.committed.composited_rgba.as_raw(),
        reference.composited_rgba.as_raw(),
        "committed compositor pixels must match full export bit-for-bit"
    );
}

#[test]
fn incremental_blur_commit_preserves_exactness_and_reuses_unaffected_tiles() {
    // Wider than DISPLAY_TILE_EDGE (512) so the image splits into two horizontal display tiles.
    let source = patterned_image(560, 64);
    let cancel = CancellationToken::new();

    let base_rect = AnnotationItem {
        id: AnnotationId(1),
        kind: AnnotationKind::Rect {
            rect: Rect2::new(24.0, 10.0, 96.0, 40.0),
            stroke: RgbaColor::GREEN,
            stroke_width: 3.0,
            fill: None,
        },
    };
    let initial = Request {
        source: source.clone(),
        transform: TransformState::default(),
        committed: Arc::from([base_rect.clone()]),
        transient: None,
        svg: None,
    };
    let (compositor, initial_payload) = initial.compose(None, &cancel).expect("initial composite");
    assert_eq!(initial_payload.tiles.len(), 2);

    let blur_step_1 = AnnotationItem {
        id: AnnotationId(99),
        kind: AnnotationKind::GaussianBlur {
            region: Rect2::new(40.0, 12.0, 64.0, 32.0),
            radius: 4,
        },
    };
    let drag_1 = Request {
        source: source.clone(),
        transform: TransformState::default(),
        committed: Arc::from([base_rect.clone()]),
        transient: Some((1, Arc::from([base_rect.clone(), blur_step_1]))),
        svg: None,
    };
    let (compositor, drag_1_payload) = drag_1
        .compose(Some(compositor), &cancel)
        .expect("blur drag step 1");
    assert!(
        Arc::ptr_eq(&initial_payload.tiles[1].1, &drag_1_payload.tiles[1].1),
        "unaffected right tile must be reused during blur drag"
    );

    let blur_step_2 = AnnotationItem {
        id: AnnotationId(99),
        kind: AnnotationKind::GaussianBlur {
            region: Rect2::new(40.0, 12.0, 120.0, 40.0),
            radius: 4,
        },
    };
    let drag_2 = Request {
        source: source.clone(),
        transform: TransformState::default(),
        committed: Arc::from([base_rect.clone()]),
        transient: Some((1, Arc::from([base_rect.clone(), blur_step_2.clone()]))),
        svg: None,
    };
    let (compositor, drag_2_payload) = drag_2
        .compose(Some(compositor), &cancel)
        .expect("blur drag step 2");
    assert!(Arc::ptr_eq(
        &initial_payload.tiles[1].1,
        &drag_2_payload.tiles[1].1
    ));

    let committed_blur = AnnotationItem {
        id: AnnotationId(2),
        kind: blur_step_2.kind,
    };
    let committed_items: Arc<[AnnotationItem]> = Arc::from([base_rect, committed_blur]);
    let commit = Request {
        source: source.clone(),
        transform: TransformState::default(),
        committed: committed_items.clone(),
        transient: None,
        svg: None,
    };
    let (compositor, commit_payload) = commit
        .compose(Some(compositor), &cancel)
        .expect("blur commit");

    assert!(compositor.exact);
    assert!(
        Arc::ptr_eq(&drag_2_payload.tiles[0].1, &commit_payload.tiles[0].1),
        "unchanged blur tile must be reused directly on release without re-copying"
    );
    assert!(
        Arc::ptr_eq(&initial_payload.tiles[1].1, &commit_payload.tiles[1].1),
        "unaffected right tile must stay shared across commit"
    );

    let reference = build_render_textures(
        source,
        None,
        TransformState::default(),
        &committed_items,
        &cancel,
    )
    .expect("reference blur build");
    assert_eq!(
        compositor.committed.composited_rgba.as_raw(),
        reference.composited_rgba.as_raw(),
        "topmost blur commit must match full export bit-for-bit"
    );
}

#[test]
fn complete_frame_install_is_gated_against_newer_gestures_and_active_edits() {
    let matching_complete = decide_frame_install(FrameInstallCheck {
        doc_revision: 5,
        rendered_revision: 5,
        current_gesture: 3,
        rendered_gesture: 3,
        is_preview: false,
        compositor_exact: true,
        pointer_gesture_active: false,
        editing_text: false,
    });
    assert_eq!(
        matching_complete,
        FrameInstallDecision {
            cache_complete: true,
            install_frame: true,
        }
    );

    // Late Complete render arriving after a new drag started: cache complete pixels for export,
    // but never overwrite the live drag frame or clear `preview_objects`.
    let late_complete_during_drag = decide_frame_install(FrameInstallCheck {
        doc_revision: 5,
        rendered_revision: 5,
        current_gesture: 4,
        rendered_gesture: 3,
        is_preview: false,
        compositor_exact: true,
        pointer_gesture_active: true,
        editing_text: false,
    });
    assert_eq!(
        late_complete_during_drag,
        FrameInstallDecision {
            cache_complete: true,
            install_frame: false,
        }
    );

    // Late Complete render while editing text must not clobber the live text preview either.
    let late_complete_during_text = decide_frame_install(FrameInstallCheck {
        doc_revision: 5,
        rendered_revision: 5,
        current_gesture: 3,
        rendered_gesture: 3,
        is_preview: false,
        compositor_exact: true,
        pointer_gesture_active: false,
        editing_text: true,
    });
    assert_eq!(
        late_complete_during_text,
        FrameInstallDecision {
            cache_complete: true,
            install_frame: false,
        }
    );

    // Active preview frame installs without polluting the authoritative complete export cache.
    let active_preview = decide_frame_install(FrameInstallCheck {
        doc_revision: 5,
        rendered_revision: 5,
        current_gesture: 4,
        rendered_gesture: 4,
        is_preview: true,
        compositor_exact: true,
        pointer_gesture_active: true,
        editing_text: false,
    });
    assert_eq!(
        active_preview,
        FrameInstallDecision {
            cache_complete: false,
            install_frame: true,
        }
    );

    // Stale revision is rejected for both cache and frame installation.
    let stale_revision = decide_frame_install(FrameInstallCheck {
        doc_revision: 6,
        rendered_revision: 5,
        current_gesture: 4,
        rendered_gesture: 4,
        is_preview: false,
        compositor_exact: true,
        pointer_gesture_active: false,
        editing_text: false,
    });
    assert_eq!(
        stale_revision,
        FrameInstallDecision {
            cache_complete: false,
            install_frame: false,
        }
    );
}

#[test]
fn svg_zoom_quantization_and_reuse_skip_tiny_scale_changes() {
    assert_eq!(quantize_svg_scale(1.0), None);
    assert_eq!(quantize_svg_scale(1.05), None);
    let s1 = quantize_svg_scale(2.02);
    let s2 = quantize_svg_scale(2.08);
    let s3 = quantize_svg_scale(2.20);
    assert!(s1.is_some());
    assert!(svg_scale_matches(s1, s2));
    assert!(!svg_scale_matches(s1, s3));
    assert!(!svg_scale_matches(None, s1));
    assert!(!svg_scale_matches(s1, None));
}

#[test]
fn svg_zoom_reuses_source_buffers_and_skips_rebuild_within_tolerance() {
    let svg_bytes: Arc<[u8]> = Arc::from(
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24"><rect width="32" height="24" fill="#ef4444"/></svg>"##.as_slice(),
    );
    let source = patterned_image(32, 24);
    let cancel = CancellationToken::new();

    let zoom_1 = quantize_svg_scale(2.0).expect("zoom above threshold");
    let first = Request {
        source: source.clone(),
        transform: TransformState::default(),
        committed: Arc::from([]),
        transient: None,
        svg: Some((svg_bytes.clone(), zoom_1)),
    };
    let (compositor, payload_1) = first.compose(None, &cancel).expect("svg zoom composite");
    assert!(compositor.exact);
    assert!(Arc::ptr_eq(&compositor.committed.composited_rgba, &source));
    assert!(Arc::ptr_eq(&compositor.committed.transformed_base, &source));
    assert_eq!((payload_1.width, payload_1.height), (64, 48));

    let zoom_2 = quantize_svg_scale(2.06).expect("zoom within 5% tolerance");
    let second = Request {
        source: source.clone(),
        transform: TransformState::default(),
        committed: Arc::from([]),
        transient: None,
        svg: Some((svg_bytes, zoom_2)),
    };
    let (compositor, payload_2) = second
        .compose(Some(compositor), &cancel)
        .expect("reused svg zoom composite");
    assert!(compositor.exact);
    assert!(
        Arc::ptr_eq(&payload_1.tiles[0].1, &payload_2.tiles[0].1),
        "tiny SVG zoom delta within tolerance must reuse cached tiles without re-rasterizing"
    );
}
