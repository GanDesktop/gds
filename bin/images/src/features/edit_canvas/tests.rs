use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use gds::service::image::{
    PixelRect,
    decode::DecodedImage,
    metadata::{EditableFields, ExifSummary, MetadataBundle, TechnicalFacts},
    ops::OrthogonalRotation,
    source::{Format, MemoryOrigin, Origin},
};
use image::{Rgba, RgbaImage};

use super::{
    ActiveInteraction, CoalesceKey, EditCommand, EditHistory, FinishOutcome, HandleKind, HitTarget,
    HoverTarget, MAX_UNDO_DEPTH, PointerDownInput, PointerModifiers, PointerMoveOutcome,
    PropertyValue, ReorderOp, ResizeModifiers, SelectionSet, ToolFsm, ToolMode, handles_for_item,
    hit_test_canvas, hit_test_cycle_stack, hover_test_canvas, marquee_hits, reorder_annotations,
    resize_item_with_handle, translate_group,
};
use crate::{
    entities::document::{
        AnnotationId, AnnotationItem, AnnotationKind, DocumentState, RgbaColor, SpatialTransformOp,
    },
    shared::geometry::{Point2, Rect2},
};

fn dummy_document(width: u32, height: u32) -> DocumentState {
    let mut rgba = RgbaImage::new(width, height);
    rgba.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
    let decoded = DecodedImage {
        origin: Origin::Memory {
            kind: MemoryOrigin::Clipboard,
            bytes: Arc::from([]),
            format_hint: Some(Format::Png),
        },
        format: Format::Png,
        rgba: Arc::new(rgba),
        metadata: MetadataBundle {
            technical: TechnicalFacts {
                format: Format::Png,
                width,
                height,
                file_bytes: Some(0),
                color_type: "Rgba8".to_owned(),
                has_alpha: true,
                orientation: 1,
                icc_profile_name: None,
                has_icc_profile: false,
                has_embedded_thumbnail: false,
                frame_count: None,
            },
            exif: ExifSummary::default(),
            editable: EditableFields::default(),
            raw_exif: None,
            raw_icc: None,
        },
        was_color_managed: false,
        is_preview_downscaled: false,
        source_bytes: Arc::from([]),
    };
    DocumentState::from_decoded(decoded)
}

fn push_n_flips(doc: &mut DocumentState, history: &mut EditHistory, count: usize) {
    for _ in 0..count {
        let cmd = EditCommand::build_spatial_transform(SpatialTransformOp::FlipHorizontal, doc);
        history.apply(cmd, doc);
    }
}

fn count_undos(doc: &mut DocumentState, history: &mut EditHistory) -> usize {
    let mut n = 0;
    while history.undo(doc) {
        n += 1;
    }
    n
}

fn rect_item(id: u64, rect: Rect2, color: RgbaColor, fill: Option<RgbaColor>) -> AnnotationItem {
    AnnotationItem {
        id: AnnotationId(id),
        kind: AnnotationKind::Rect {
            rect,
            stroke: color,
            stroke_width: 4.0,
            fill,
        },
    }
}

fn text_item(id: u64, pos: [f32; 2], content: &str, font_size: f32) -> AnnotationItem {
    AnnotationItem {
        id: AnnotationId(id),
        kind: AnnotationKind::Text {
            family: "sans-serif".into(),
            position: pos,
            content: content.to_owned(),
            color: RgbaColor::BLUE,
            font_size,
        },
    }
}

#[test]
fn edit_history_preserves_annotations_across_transforms_and_crop_undo() {
    let mut doc = dummy_document(200, 120);
    let mut history = EditHistory::default();
    let mut fsm = ToolFsm::default();

    let inside = rect_item(1, Rect2::new(30.0, 30.0, 40.0, 30.0), RgbaColor::RED, None);
    let crossing = rect_item(
        2,
        Rect2::new(10.0, 40.0, 30.0, 20.0),
        RgbaColor::GREEN,
        None,
    );
    let outside = rect_item(
        3,
        Rect2::new(140.0, 80.0, 30.0, 20.0),
        RgbaColor::BLUE,
        None,
    );
    let label = text_item(4, [40.0, 45.0], "Keep me", 18.0);
    history.apply(
        EditCommand::AddAnnotations(Arc::from([inside, crossing, outside, label])),
        &mut doc,
    );
    doc.next_annotation_id = 5;
    fsm.selection = SelectionSet::from_ids([AnnotationId(1), AnnotationId(3)]);

    fsm.apply_spatial_transform(SpatialTransformOp::RotateCw, &mut doc, &mut history);
    assert_eq!(doc.transform.spec.rotation, OrthogonalRotation::Deg90);
    assert_eq!(doc.output_dimensions(), (120, 200));
    assert_eq!(doc.annotations.len(), 4);
    assert_eq!(
        doc.annotations[0].visual_bounds(),
        Rect2::new(60.0, 30.0, 30.0, 40.0)
    );

    assert!(history.undo(&mut doc));
    assert_eq!(doc.output_dimensions(), (200, 120));
    assert_eq!(
        doc.annotations[0].visual_bounds(),
        Rect2::new(30.0, 30.0, 40.0, 30.0)
    );

    // Crop [20..120, 20..100] excludes `outside` while keeping `inside`, `crossing`, `label`.
    fsm.set_tool(ToolMode::Crop, &mut history);
    fsm.on_pointer_down(
        &doc,
        &mut history,
        PointerDownInput {
            vp_pt: Point2::new(20.0, 20.0),
            img_pt: Point2::new(20.0, 20.0),
            scale: 1.0,
            modifiers: PointerModifiers::default(),
            click_count: 1,
        },
    );
    fsm.on_pointer_move(
        &doc,
        Point2::new(120.0, 100.0),
        Point2::new(120.0, 100.0),
        PointerModifiers::default(),
    );
    assert_eq!(
        fsm.on_pointer_up(&mut doc, &mut history, 1.0),
        FinishOutcome::CropStaged(PixelRect::new(20, 20, 100, 80))
    );
    fsm.selection = SelectionSet::from_ids([AnnotationId(1), AnnotationId(3)]);
    assert!(fsm.apply_staged_crop(&mut doc, &mut history));
    assert_eq!(doc.output_dimensions(), (100, 80));
    assert_eq!(doc.annotations.len(), 3);
    assert!(!fsm.selection.contains(AnnotationId(3)));

    // Undo crop restores all 4 annotations including the cropped-out item.
    assert!(history.undo(&mut doc));
    assert_eq!(doc.output_dimensions(), (200, 120));
    assert_eq!(doc.annotations.len(), 4);

    // 4 clockwise rotations return to identity and clear dirty flag.
    let mut clean_doc = dummy_document(40, 20);
    let mut rot_history = EditHistory::default();
    let mut rot_fsm = ToolFsm::default();
    rot_fsm.apply_spatial_transform(
        SpatialTransformOp::RotateCw,
        &mut clean_doc,
        &mut rot_history,
    );
    rot_fsm.apply_spatial_transform(
        SpatialTransformOp::RotateCw,
        &mut clean_doc,
        &mut rot_history,
    );
    rot_fsm.apply_spatial_transform(
        SpatialTransformOp::RotateCw,
        &mut clean_doc,
        &mut rot_history,
    );
    rot_fsm.apply_spatial_transform(
        SpatialTransformOp::RotateCw,
        &mut clean_doc,
        &mut rot_history,
    );
    assert!(!clean_doc.has_unbaked_ops());
    assert!(!clean_doc.is_dirty());
}

#[test]
fn tool_fsm_drag_rejects_off_canvas_arrows_and_preserves_annotation_ids() {
    let mut doc = dummy_document(120, 90);
    let mut history = EditHistory::default();
    let mut fsm = ToolFsm::default();

    fsm.set_tool(ToolMode::Arrow, &mut history);
    fsm.on_pointer_down(
        &doc,
        &mut history,
        PointerDownInput {
            vp_pt: Point2::new(-50.0, 10.0),
            img_pt: Point2::new(-50.0, 10.0),
            scale: 1.0,
            modifiers: PointerModifiers::default(),
            click_count: 1,
        },
    );
    fsm.on_pointer_move(
        &doc,
        Point2::new(-10.0, 70.0),
        Point2::new(-10.0, 70.0),
        PointerModifiers::default(),
    );
    assert_eq!(
        fsm.on_pointer_up(&mut doc, &mut history, 1.0),
        FinishOutcome::None
    );
    assert_eq!(doc.next_annotation_id, 1);

    fsm.set_tool(ToolMode::Line, &mut history);
    fsm.on_pointer_down(
        &doc,
        &mut history,
        PointerDownInput {
            vp_pt: Point2::new(-20.0, 45.0),
            img_pt: Point2::new(-20.0, 45.0),
            scale: 1.0,
            modifiers: PointerModifiers::default(),
            click_count: 1,
        },
    );
    fsm.on_pointer_move(
        &doc,
        Point2::new(60.0, 45.0),
        Point2::new(60.0, 45.0),
        PointerModifiers::default(),
    );
    assert_eq!(
        fsm.on_pointer_up(&mut doc, &mut history, 1.0),
        FinishOutcome::Committed
    );
    assert_eq!(doc.next_annotation_id, 2);
    assert_eq!(fsm.tool, ToolMode::Line);
    assert_eq!(fsm.selection.primary(), Some(AnnotationId(1)));
    assert_eq!(
        doc.annotations[0].kind,
        AnnotationKind::Line {
            start: [-20.0, 45.0],
            end: [60.0, 45.0],
            color: RgbaColor::RED,
            stroke_width: 3.0,
        }
    );
}

#[test]
fn edit_history_enforces_max_undo_depth() {
    let mut doc = dummy_document(10, 10);
    let mut history = EditHistory::default();
    push_n_flips(&mut doc, &mut history, MAX_UNDO_DEPTH + 10);
    assert_eq!(count_undos(&mut doc, &mut history), MAX_UNDO_DEPTH);
}

#[test]
fn hybrid_hit_testing_and_hover_obey_tool_and_screen_scale_rules() {
    let hollow = rect_item(1, Rect2::new(20.0, 20.0, 100.0, 80.0), RgbaColor::RED, None);
    let nested_text = text_item(2, [45.0, 50.0], "Inner", 18.0);
    let mosaic = AnnotationItem {
        id: AnnotationId(3),
        kind: AnnotationKind::Mosaic {
            region: Rect2::new(140.0, 20.0, 60.0, 60.0),
            block_size: 10,
        },
    };
    let annotations = vec![hollow, nested_text, mosaic];
    let empty_sel = SelectionSet::empty();

    // Hollow interior passes through to nested text; blank interior in Rect mode misses hollow.
    assert_eq!(
        hit_test_canvas(
            &annotations,
            &empty_sel,
            Point2::new(90.0, 75.0),
            ToolMode::Rect,
            1.0
        ),
        None
    );
    // Drawing over an existing stroke starts a new shape.
    assert_eq!(
        hit_test_canvas(
            &annotations,
            &empty_sel,
            Point2::new(60.0, 20.0),
            ToolMode::Rect,
            1.0
        ),
        None
    );
    // Nested text inside hollow rect is directly clickable.
    let text_center = annotations[1].visual_bounds().center();
    assert_eq!(
        hit_test_canvas(
            &annotations,
            &empty_sel,
            text_center,
            ToolMode::Pointer,
            1.0
        ),
        Some(HitTarget::Body {
            id: AnnotationId(2)
        })
    );

    // The pointer discovers redactions. Drawing tools pass through their interior and border.
    assert_eq!(
        hover_test_canvas(
            &annotations,
            &empty_sel,
            Point2::new(170.0, 50.0),
            ToolMode::Pointer,
            1.0
        ),
        HoverTarget::Body {
            id: AnnotationId(3),
            discover_redaction: true,
        }
    );
    assert_eq!(
        hit_test_canvas(
            &annotations,
            &empty_sel,
            Point2::new(170.0, 50.0),
            ToolMode::Mosaic,
            1.0
        ),
        None
    );
    assert_eq!(
        hit_test_canvas(
            &annotations,
            &empty_sel,
            Point2::new(140.0, 50.0),
            ToolMode::Mosaic,
            1.0
        ),
        None
    );
    assert_eq!(
        hit_test_canvas(
            &annotations,
            &empty_sel,
            Point2::new(140.0, 50.0),
            ToolMode::Arrow,
            1.0
        ),
        None
    );

    // Single-selection handle priority wins over body and scales inversely with zoom.
    let sel_one = SelectionSet::single(AnnotationId(1));
    assert_eq!(
        hit_test_canvas(
            &annotations,
            &sel_one,
            Point2::new(70.0, 20.0),
            ToolMode::Pointer,
            1.0
        ),
        Some(HitTarget::Handle {
            id: AnnotationId(1),
            handle: HandleKind::N,
        })
    );
    // At 4x zoom, 7.0/4.0 = 1.75px handle radius does not trigger 5px away from NW corner.
    assert_eq!(
        hit_test_canvas(
            &annotations,
            &sel_one,
            Point2::new(25.0, 20.0),
            ToolMode::Pointer,
            4.0
        ),
        Some(HitTarget::Body {
            id: AnnotationId(1)
        })
    );

    // Cycle stack collects top-to-bottom overlapping hits; marquee collects intersecting objects.
    let stack_rect = rect_item(4, Rect2::new(140.0, 20.0, 60.0, 60.0), RgbaColor::RED, None);
    let stacked = vec![annotations[2].clone(), stack_rect];
    assert_eq!(
        hit_test_cycle_stack(&stacked, Point2::new(140.0, 50.0), 1.0),
        vec![AnnotationId(4), AnnotationId(3)]
    );
    // When lower overlapping Mosaic is already selected, hit_test_canvas keeps it draggable
    // instead of letting the unselected upper Rect shadow it, while Alt+Click stack stays ordered.
    let sel_lower = SelectionSet::single(AnnotationId(3));
    assert_eq!(
        hit_test_canvas(
            &stacked,
            &empty_sel,
            Point2::new(140.0, 35.0),
            ToolMode::Pointer,
            1.0
        ),
        Some(HitTarget::Body {
            id: AnnotationId(4)
        })
    );
    assert_eq!(
        hit_test_canvas(
            &stacked,
            &sel_lower,
            Point2::new(140.0, 35.0),
            ToolMode::Pointer,
            1.0
        ),
        Some(HitTarget::Body {
            id: AnnotationId(3)
        })
    );
    assert_eq!(
        hit_test_cycle_stack(&stacked, Point2::new(140.0, 35.0), 1.0),
        vec![AnnotationId(4), AnnotationId(3)]
    );
    assert_eq!(
        marquee_hits(&annotations, Rect2::new(15.0, 15.0, 40.0, 40.0)),
        vec![AnnotationId(1), AnnotationId(2)]
    );
}

#[test]
fn group_translation_preserves_off_canvas_geometry_and_handles_resize_correctly() {
    let left_rect = rect_item(1, Rect2::new(10.0, 20.0, 20.0, 20.0), RgbaColor::RED, None);
    let right_rect = rect_item(2, Rect2::new(60.0, 20.0, 20.0, 20.0), RgbaColor::BLUE, None);
    let (moved, dx, dy) = translate_group(&[left_rect, right_rect], -50.0, 15.0, false);
    assert_eq!((dx, dy), (-50.0, 15.0));
    assert_eq!(moved[0].visual_bounds().x, -40.0);
    assert_eq!(moved[1].visual_bounds().x, 10.0);
    assert_eq!(
        moved[1].visual_bounds().x - moved[0].visual_bounds().right(),
        30.0
    );

    // Axis-flip across opposite corner when dragging Se past Nw, plus Shift 1:1 lock.
    let box_item = rect_item(3, Rect2::new(40.0, 40.0, 30.0, 20.0), RgbaColor::RED, None);
    let flipped = resize_item_with_handle(
        &box_item,
        HandleKind::Se,
        Point2::new(10.0, 20.0),
        ResizeModifiers {
            shift: true,
            ctrl: false,
            alt: false,
        },
    );
    assert_eq!(flipped.visual_bounds(), Rect2::new(10.0, 10.0, 30.0, 30.0));

    // Line endpoint 15-degree snap.
    let line = AnnotationItem {
        id: AnnotationId(4),
        kind: AnnotationKind::Line {
            start: [50.0, 50.0],
            end: [100.0, 50.0],
            color: RgbaColor::RED,
            stroke_width: 2.0,
        },
    };
    let snapped = resize_item_with_handle(
        &line,
        HandleKind::LineEnd,
        Point2::new(90.0, 52.0),
        ResizeModifiers {
            shift: true,
            ctrl: false,
            alt: false,
        },
    );
    assert_eq!(
        snapped.kind,
        AnnotationKind::Line {
            start: [50.0, 50.0],
            end: [90.0, 50.0],
            color: RgbaColor::RED,
            stroke_width: 2.0,
        }
    );

    // Mixed Latin/CJK multiline Text resize via Nw keeps Se corner stationary.
    let mixed_text = text_item(5, [60.0, 60.0], "iiii WWWW\n中文标注", 24.0);
    let orig_bounds = mixed_text.visual_bounds();
    let orig_se = Point2::new(orig_bounds.right(), orig_bounds.bottom());
    let resized_text = resize_item_with_handle(
        &mixed_text,
        HandleKind::Nw,
        Point2::new(20.0, 20.0),
        ResizeModifiers::default(),
    );
    let next_bounds = resized_text.visual_bounds();
    assert!((next_bounds.right() - orig_se.x).abs() < 0.25);
    assert!((next_bounds.bottom() - orig_se.y).abs() < 0.25);
    assert!(!handles_for_item(&resized_text).is_empty());
}

#[test]
fn reorder_annotations_preserves_multiselection_relative_order() {
    let items = vec![
        rect_item(1, Rect2::new(0.0, 0.0, 10.0, 10.0), RgbaColor::RED, None),
        rect_item(2, Rect2::new(0.0, 0.0, 10.0, 10.0), RgbaColor::GREEN, None),
        rect_item(3, Rect2::new(0.0, 0.0, 10.0, 10.0), RgbaColor::BLUE, None),
        rect_item(4, Rect2::new(0.0, 0.0, 10.0, 10.0), RgbaColor::WHITE, None),
        rect_item(5, Rect2::new(0.0, 0.0, 10.0, 10.0), RgbaColor::BLACK, None),
    ];
    let sel = SelectionSet::from_ids([AnnotationId(2), AnnotationId(4)]);

    assert_eq!(
        reorder_annotations(&items, &sel, ReorderOp::BringForward),
        Some(vec![
            AnnotationId(1),
            AnnotationId(3),
            AnnotationId(2),
            AnnotationId(5),
            AnnotationId(4),
        ])
    );
    assert_eq!(
        reorder_annotations(&items, &sel, ReorderOp::SendToBack),
        Some(vec![
            AnnotationId(2),
            AnnotationId(4),
            AnnotationId(1),
            AnnotationId(3),
            AnnotationId(5),
        ])
    );

    let mut doc = dummy_document(100, 100);
    doc.annotations = items;
    let mut history = EditHistory::default();
    let mut fsm = ToolFsm {
        selection: sel,
        ..Default::default()
    };
    assert!(fsm.reorder_selection(ReorderOp::BringToFront, &mut doc, &mut history));
    assert_eq!(
        doc.annotations.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![
            AnnotationId(1),
            AnnotationId(3),
            AnnotationId(5),
            AnnotationId(2),
            AnnotationId(4),
        ]
    );
    assert!(history.undo(&mut doc));
    assert_eq!(
        doc.annotations.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![
            AnnotationId(1),
            AnnotationId(2),
            AnnotationId(3),
            AnnotationId(4),
            AnnotationId(5),
        ]
    );
}

#[test]
fn history_coalescing_breaks_at_boundaries_and_merges_within_window() {
    let mut doc = dummy_document(100, 100);
    doc.annotations = vec![
        rect_item(1, Rect2::new(10.0, 10.0, 20.0, 20.0), RgbaColor::RED, None),
        rect_item(2, Rect2::new(40.0, 40.0, 20.0, 20.0), RgbaColor::BLUE, None),
    ];
    let mut history = EditHistory::default();
    let t0 = Instant::now();
    let key_a = CoalesceKey::KeyboardNudge { selection_hash: 11 };
    let key_b = CoalesceKey::KeyboardNudge { selection_hash: 22 };

    let step1 = doc.annotations[0].translated(1.0, 0.0);
    let step2 = step1.translated(1.0, 0.0);
    history.apply_coalesced_at(
        key_a,
        EditCommand::UpdateAnnotations {
            before: Arc::from([doc.annotations[0].clone()]),
            after: Arc::from([step1.clone()]),
        },
        &mut doc,
        t0,
    );
    history.apply_coalesced_at(
        key_a,
        EditCommand::UpdateAnnotations {
            before: Arc::from([step1.clone()]),
            after: Arc::from([step2.clone()]),
        },
        &mut doc,
        t0 + Duration::from_millis(200),
    );
    // Explicitly breaking coalesce (as when switching selection A -> B -> A) prevents merge.
    history.break_coalesce();
    let step3 = step2.translated(1.0, 0.0);
    history.apply_coalesced_at(
        key_a,
        EditCommand::UpdateAnnotations {
            before: Arc::from([step2]),
            after: Arc::from([step3.clone()]),
        },
        &mut doc,
        t0 + Duration::from_millis(300),
    );
    // Switching key breaks merge; exceeding 600ms also breaks merge.
    let step4 = doc.annotations[1].translated(0.0, 1.0);
    history.apply_coalesced_at(
        key_b,
        EditCommand::UpdateAnnotations {
            before: Arc::from([doc.annotations[1].clone()]),
            after: Arc::from([step4.clone()]),
        },
        &mut doc,
        t0 + Duration::from_millis(350),
    );
    let step5 = step4.translated(0.0, 1.0);
    history.apply_coalesced_at(
        key_b,
        EditCommand::UpdateAnnotations {
            before: Arc::from([step4]),
            after: Arc::from([step5]),
        },
        &mut doc,
        t0 + Duration::from_millis(1000),
    );

    assert_eq!(count_undos(&mut doc, &mut history), 4);
    assert_eq!(doc.annotations[0].visual_bounds().x, 10.0);
    assert_eq!(doc.annotations[1].visual_bounds().y, 40.0);
}

#[test]
fn text_draft_and_mixed_property_summary_keep_transactions_and_defaults_clean() {
    let mut doc = dummy_document(300, 200);
    let mut history = EditHistory::default();
    let mut fsm = ToolFsm::default();

    let r1 = rect_item(1, Rect2::new(10.0, 10.0, 30.0, 30.0), RgbaColor::RED, None);
    let m2 = AnnotationItem {
        id: AnnotationId(2),
        kind: AnnotationKind::Mosaic {
            region: Rect2::new(60.0, 10.0, 30.0, 30.0),
            block_size: 14,
        },
    };
    let r3 = rect_item(
        3,
        Rect2::new(110.0, 10.0, 30.0, 30.0),
        RgbaColor::BLUE,
        None,
    );
    let t4 = text_item(4, [20.0, 80.0], "Original", 20.0);
    doc.annotations = vec![r1, m2, r3, t4];
    doc.next_annotation_id = 5;

    // Selecting items reads Mixed / Uniform / Unsupported without overwriting fsm.settings.
    let initial_settings = fsm.settings.clone();
    fsm.selection = SelectionSet::from_ids([AnnotationId(1), AnnotationId(2), AnnotationId(3)]);
    let summary = fsm
        .selection_style_summary(&doc.annotations)
        .expect("non-empty selection summary");
    assert_eq!(summary.count, 3);
    assert_eq!(summary.color, PropertyValue::Mixed);
    assert_eq!(summary.color_applicable, 2);
    assert_eq!(summary.stroke_width, PropertyValue::Uniform(4.0));
    assert_eq!(summary.stroke_applicable, 2);
    assert_eq!(summary.mosaic_block_size, PropertyValue::Uniform(14));
    assert_eq!(summary.mosaic_applicable, 1);
    assert_eq!(summary.font_size, PropertyValue::Unsupported);
    assert_eq!(fsm.settings, initial_settings);

    // Double-click / Enter text edit keeps content + color in a single cancelable/undoable draft.
    assert_eq!(
        fsm.begin_edit_existing_text(AnnotationId(4), &doc, &mut history),
        Some("Original".to_owned())
    );
    assert!(fsm.update_text_draft_content("Updated".to_owned()));
    fsm.apply_color(RgbaColor::GREEN, Some(&mut doc), &mut history);
    assert!(!history.can_undo());

    // Cancel discards both text edit and color edit cleanly.
    fsm.cancel_active(&mut history);
    assert_eq!(
        doc.annotations[3],
        text_item(4, [20.0, 80.0], "Original", 20.0)
    );
    assert!(!history.can_undo());

    assert_eq!(fsm.settings, initial_settings);

    // Re-open, change both text and color, commit -> 1 single undo restores both together.
    fsm.begin_edit_existing_text(AnnotationId(4), &doc, &mut history);
    fsm.apply_color(RgbaColor::AMBER, Some(&mut doc), &mut history);
    assert_eq!(
        fsm.commit_text_draft("Final\nLine 2", &mut doc, &mut history),
        FinishOutcome::Committed
    );
    assert!(history.undo(&mut doc));
    assert_eq!(
        doc.annotations[3],
        text_item(4, [20.0, 80.0], "Original", 20.0)
    );
    assert!(!history.undo(&mut doc));
}

#[test]
fn pointer_threshold_alt_drag_duplicate_and_safe_delete_behave_per_spec() {
    let mut doc = dummy_document(200, 200);
    let mut history = EditHistory::default();
    let mut fsm = ToolFsm::default();
    doc.annotations = vec![rect_item(
        1,
        Rect2::new(20.0, 20.0, 40.0, 40.0),
        RgbaColor::RED,
        None,
    )];
    doc.next_annotation_id = 2;

    // Click under 3px threshold selects without creating history.
    fsm.on_pointer_down(
        &doc,
        &mut history,
        PointerDownInput {
            vp_pt: Point2::new(30.0, 20.0),
            img_pt: Point2::new(30.0, 20.0),
            scale: 1.0,
            modifiers: PointerModifiers::default(),
            click_count: 1,
        },
    );
    assert_eq!(
        fsm.on_pointer_move(
            &doc,
            Point2::new(31.5, 20.0),
            Point2::new(31.5, 20.0),
            PointerModifiers::default(),
        ),
        PointerMoveOutcome::None
    );
    assert_eq!(
        fsm.on_pointer_up(&mut doc, &mut history, 1.0),
        FinishOutcome::SelectionChanged
    );
    assert_eq!(fsm.selection, SelectionSet::single(AnnotationId(1)));
    assert!(!history.can_undo());

    // Alt+Drag across 3px threshold enters DuplicatingMove and commits clone on release.
    let alt_mods = PointerModifiers {
        alt: true,
        ..Default::default()
    };
    fsm.on_pointer_down(
        &doc,
        &mut history,
        PointerDownInput {
            vp_pt: Point2::new(30.0, 20.0),
            img_pt: Point2::new(30.0, 20.0),
            scale: 1.0,
            modifiers: alt_mods,
            click_count: 1,
        },
    );
    assert!(matches!(
        fsm.on_pointer_move(
            &doc,
            Point2::new(50.0, 40.0),
            Point2::new(50.0, 40.0),
            alt_mods
        ),
        PointerMoveOutcome::NeedsTransientPreview {
            first_changed_idx: 1
        }
    ));
    assert!(matches!(
        fsm.interaction,
        ActiveInteraction::DuplicatingMove { .. }
    ));
    assert_eq!(
        fsm.on_pointer_up(&mut doc, &mut history, 1.0),
        FinishOutcome::Committed
    );
    assert_eq!(doc.annotations.len(), 2);
    assert_eq!(fsm.selection, SelectionSet::single(AnnotationId(2)));

    // Ctrl+D duplicate and safe delete down to empty selection.
    assert_eq!(
        fsm.duplicate_selected(&mut doc, &mut history),
        FinishOutcome::Committed
    );
    assert_eq!(doc.annotations.len(), 3);
    assert!(fsm.select_all(&doc, &mut history));
    assert_eq!(
        fsm.delete_selected_or_crop(&mut doc, &mut history),
        FinishOutcome::Committed
    );
    assert!(doc.annotations.is_empty());
    assert!(fsm.selection.is_empty());
    // Repeated Delete on empty selection is a strict no-op.
    assert_eq!(
        fsm.delete_selected_or_crop(&mut doc, &mut history),
        FinishOutcome::None
    );
}

#[test]
fn brush_tool_draws_freehand_strokes_dots_hit_tests_and_resizes() {
    let mut doc = dummy_document(320, 200);
    let mut history = EditHistory::default();
    let mut fsm = ToolFsm::default();
    let none = PointerModifiers::default();

    fsm.set_tool(ToolMode::Brush, &mut history);
    assert!(ToolMode::Brush.is_configurable());
    fsm.apply_color(RgbaColor::GREEN, Some(&mut doc), &mut history);
    fsm.apply_stroke_width(6.0, Some(&mut doc), &mut history);

    // Freehand drag across 3 points with live transient preview.
    fsm.on_pointer_down(
        &doc,
        &mut history,
        PointerDownInput {
            vp_pt: Point2::new(20.0, 20.0),
            img_pt: Point2::new(20.0, 20.0),
            scale: 1.0,
            modifiers: none,
            click_count: 1,
        },
    );
    assert!(matches!(
        fsm.on_pointer_move(
            &doc,
            Point2::new(20.0, 100.0),
            Point2::new(20.0, 100.0),
            none
        ),
        PointerMoveOutcome::NeedsTransientPreview {
            first_changed_idx: 0
        }
    ));
    fsm.on_pointer_move(
        &doc,
        Point2::new(100.0, 100.0),
        Point2::new(100.0, 100.0),
        none,
    );
    let (_, preview) = fsm
        .transient_annotations(&doc)
        .expect("active brush produces transient preview");
    assert_eq!(preview.len(), 1);
    assert_eq!(
        fsm.on_pointer_up(&mut doc, &mut history, 1.0),
        FinishOutcome::Committed
    );
    assert_eq!(doc.annotations.len(), 1);
    let brush_id = doc.annotations[0].id;
    assert_eq!(doc.annotations[0].color(), Some(RgbaColor::GREEN));
    assert_eq!(doc.annotations[0].stroke_width(), Some(6.0));

    // Single click without drag commits a dot.
    fsm.on_pointer_down(
        &doc,
        &mut history,
        PointerDownInput {
            vp_pt: Point2::new(200.0, 50.0),
            img_pt: Point2::new(200.0, 50.0),
            scale: 1.0,
            modifiers: none,
            click_count: 1,
        },
    );
    assert_eq!(
        fsm.on_pointer_up(&mut doc, &mut history, 1.0),
        FinishOutcome::Committed
    );
    assert_eq!(doc.annotations.len(), 2);
    let dot_id = doc.annotations[1].id;

    // Hit-test finds segments & dot accurately while ignoring hollow interior.
    assert_eq!(
        hit_test_canvas(
            &doc.annotations,
            &SelectionSet::empty(),
            Point2::new(20.0, 60.0),
            ToolMode::Pointer,
            1.0,
        ),
        Some(HitTarget::Body { id: brush_id })
    );
    assert_eq!(
        hit_test_canvas(
            &doc.annotations,
            &SelectionSet::empty(),
            Point2::new(75.0, 45.0),
            ToolMode::Pointer,
            1.0,
        ),
        None
    );
    assert_eq!(
        hit_test_canvas(
            &doc.annotations,
            &SelectionSet::empty(),
            Point2::new(200.0, 50.0),
            ToolMode::Pointer,
            1.0,
        ),
        Some(HitTarget::Body { id: dot_id })
    );

    // Marquee inside the empty corner misses the L-shaped stroke; marquee touching a leg hits.
    assert!(marquee_hits(&doc.annotations, Rect2::new(60.0, 30.0, 30.0, 30.0)).is_empty());
    assert_eq!(
        marquee_hits(&doc.annotations, Rect2::new(10.0, 50.0, 20.0, 20.0)),
        vec![brush_id]
    );

    // Resizing via SE handle scales points proportionally into the new bounds.
    let resized = resize_item_with_handle(
        &doc.annotations[0],
        HandleKind::Se,
        Point2::new(180.0, 180.0),
        ResizeModifiers::default(),
    );
    assert_eq!(
        resized.visual_bounds(),
        Rect2::new(20.0, 20.0, 160.0, 160.0)
    );
}
