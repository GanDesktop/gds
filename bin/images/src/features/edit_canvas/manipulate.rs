//! Pure geometry manipulation: rigid group translation, handle resizing, 15-degree snap, and reordering.

use std::ops::RangeInclusive;

use crate::{
    entities::document::{AnnotationId, AnnotationItem, AnnotationKind, measure_text_rect},
    shared::geometry::{Point2, Rect2},
};

use super::{hit_test::HandleKind, selection::SelectionSet};

const MIN_BOX_SIZE: f32 = 2.0;
pub const MARKER_DIAMETER_RANGE: RangeInclusive<f32> = 16.0..=160.0;
const MIN_LINE_LEN: f32 = 2.0;
const MIN_TEXT_SIZE: f32 = 8.0;
const MAX_TEXT_SIZE: f32 = 160.0;
const DUPLICATE_OFFSET: f32 = 16.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResizeModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReorderOp {
    BringForward,
    SendBackward,
    BringToFront,
    SendToBack,
}

pub fn translate_group(
    originals: &[AnnotationItem],
    raw_dx: f32,
    raw_dy: f32,
    shift_axis_lock: bool,
) -> (Vec<AnnotationItem>, f32, f32) {
    if originals.is_empty() {
        return (Vec::new(), 0.0, 0.0);
    }
    let (dx, dy) = if shift_axis_lock {
        if raw_dx.abs() >= raw_dy.abs() {
            (raw_dx, 0.0)
        } else {
            (0.0, raw_dy)
        }
    } else {
        (raw_dx, raw_dy)
    };

    let moved = originals
        .iter()
        .map(|item| item.translated(dx, dy))
        .collect();
    (moved, dx, dy)
}

pub fn compute_duplicate_offset(
    originals: &[AnnotationItem],
    canvas_w: u32,
    canvas_h: u32,
) -> (f32, f32) {
    let Some(union) = union_of(originals) else {
        return (0.0, 0.0);
    };
    let wf = canvas_w as f32;
    let hf = canvas_h as f32;
    let dx = axis_duplicate_offset(union.x, union.right(), wf);
    let dy = axis_duplicate_offset(union.y, union.bottom(), hf);
    (dx, dy)
}

fn axis_duplicate_offset(min: f32, max: f32, limit: f32) -> f32 {
    if max + DUPLICATE_OFFSET <= limit {
        DUPLICATE_OFFSET
    } else if min - DUPLICATE_OFFSET >= 0.0 {
        -DUPLICATE_OFFSET
    } else if limit - max >= 1.0 {
        limit - max
    } else if min >= 1.0 {
        -min
    } else {
        0.0
    }
}

pub fn resize_item_with_handle(
    original: &AnnotationItem,
    handle: HandleKind,
    target: Point2,
    modifiers: ResizeModifiers,
) -> AnnotationItem {
    let resize_region = |rect| {
        resize_box(
            rect,
            handle,
            target,
            modifiers,
            MIN_BOX_SIZE..=f32::INFINITY,
        )
    };
    let kind = match &original.kind {
        AnnotationKind::Rect {
            rect,
            stroke,
            stroke_width,
            fill,
        } => AnnotationKind::Rect {
            rect: resize_region(*rect),
            stroke: *stroke,
            stroke_width: *stroke_width,
            fill: *fill,
        },
        AnnotationKind::Ellipse {
            rect,
            stroke,
            stroke_width,
            fill,
        } => AnnotationKind::Ellipse {
            rect: resize_region(*rect),
            stroke: *stroke,
            stroke_width: *stroke_width,
            fill: *fill,
        },
        AnnotationKind::Mosaic { region, block_size } => AnnotationKind::Mosaic {
            region: resize_region(*region),
            block_size: *block_size,
        },
        AnnotationKind::GaussianBlur { region, radius } => AnnotationKind::GaussianBlur {
            region: resize_region(*region),
            radius: *radius,
        },
        AnnotationKind::Arrow {
            start,
            end,
            color,
            stroke_width,
        } => {
            let (next_start, next_end) =
                resize_segment(*start, *end, handle, target, modifiers.shift);
            AnnotationKind::Arrow {
                start: next_start,
                end: next_end,
                color: *color,
                stroke_width: *stroke_width,
            }
        }
        AnnotationKind::Line {
            start,
            end,
            color,
            stroke_width,
        } => {
            let (next_start, next_end) =
                resize_segment(*start, *end, handle, target, modifiers.shift);
            AnnotationKind::Line {
                start: next_start,
                end: next_end,
                color: *color,
                stroke_width: *stroke_width,
            }
        }
        AnnotationKind::Brush {
            points,
            color,
            stroke_width,
        } => {
            let orig_bounds = original.visual_bounds();
            let next_bounds = resize_region(orig_bounds);
            let mapped = points
                .iter()
                .map(|&pt| {
                    let nx = if orig_bounds.width > 1e-4 {
                        next_bounds.x
                            + ((pt[0] - orig_bounds.x) / orig_bounds.width) * next_bounds.width
                    } else {
                        next_bounds.x + next_bounds.width * 0.5
                    };
                    let ny = if orig_bounds.height > 1e-4 {
                        next_bounds.y
                            + ((pt[1] - orig_bounds.y) / orig_bounds.height) * next_bounds.height
                    } else {
                        next_bounds.y + next_bounds.height * 0.5
                    };
                    [nx, ny]
                })
                .collect();
            AnnotationKind::Brush {
                points: mapped,
                color: *color,
                stroke_width: *stroke_width,
            }
        }
        AnnotationKind::Number { number, color, .. } => {
            let bounds = resize_box(
                original.visual_bounds(),
                handle,
                target,
                ResizeModifiers {
                    shift: true,
                    ..modifiers
                },
                MARKER_DIAMETER_RANGE,
            );
            let radius = bounds.width * 0.5;
            AnnotationKind::Number {
                center: bounds.center(),
                radius,
                number: *number,
                color: *color,
            }
        }
        AnnotationKind::Text {
            position,
            content,
            color,
            font_size,
            family,
        } => {
            let (next_position, next_font_size) =
                resize_text(*position, content, *font_size, family, handle, target);
            AnnotationKind::Text {
                position: next_position,
                content: content.clone(),
                color: *color,
                font_size: next_font_size,
                family: family.clone(),
            }
        }
    };

    AnnotationItem {
        id: original.id,
        kind,
    }
}

pub fn snap_angle_15(fixed: Point2, moving: Point2) -> Point2 {
    let dx = moving.x - fixed.x;
    let dy = moving.y - fixed.y;
    let raw_len = dx.hypot(dy);
    if raw_len < 1e-3 {
        return moving;
    }
    let step = std::f32::consts::PI / 12.0;
    let angle = (dy.atan2(dx) / step).round() * step;
    let (uy, ux) = angle.sin_cos();
    let proj_len = (dx * ux + dy * uy).max(0.0);

    Point2::new(fixed.x + ux * proj_len, fixed.y + uy * proj_len)
}

pub fn reorder_annotations(
    annotations: &[AnnotationItem],
    selection: &SelectionSet,
    op: ReorderOp,
) -> Option<Vec<AnnotationId>> {
    if selection.is_empty() || annotations.len() < 2 {
        return None;
    }
    let original: Vec<AnnotationId> = annotations.iter().map(|item| item.id).collect();
    let mut order = original.clone();

    match op {
        ReorderOp::BringForward => {
            for i in (0..order.len().saturating_sub(1)).rev() {
                if selection.contains(order[i]) && !selection.contains(order[i + 1]) {
                    order.swap(i, i + 1);
                }
            }
        }
        ReorderOp::SendBackward => {
            for i in 1..order.len() {
                if selection.contains(order[i]) && !selection.contains(order[i - 1]) {
                    order.swap(i, i - 1);
                }
            }
        }
        ReorderOp::BringToFront => {
            let mut next = Vec::with_capacity(order.len());
            for &id in &order {
                if !selection.contains(id) {
                    next.push(id);
                }
            }
            for &id in &order {
                if selection.contains(id) {
                    next.push(id);
                }
            }
            order = next;
        }
        ReorderOp::SendToBack => {
            let mut next = Vec::with_capacity(order.len());
            for &id in &order {
                if selection.contains(id) {
                    next.push(id);
                }
            }
            for &id in &order {
                if !selection.contains(id) {
                    next.push(id);
                }
            }
            order = next;
        }
    }

    (order != original).then_some(order)
}

fn union_of(items: &[AnnotationItem]) -> Option<Rect2> {
    let mut union: Option<Rect2> = None;
    for item in items {
        let b = item.visual_bounds();
        union = Some(union.map_or(b, |acc| acc.union(b)));
    }
    union
}

fn resize_box(
    rect: Rect2,
    handle: HandleKind,
    target: Point2,
    modifiers: ResizeModifiers,
    limits: RangeInclusive<f32>,
) -> Rect2 {
    use HandleKind::*;
    let rect = rect.normalized();
    let center = rect.center();
    let moves_x = !matches!(handle, N | S);
    let moves_y = !matches!(handle, E | W);
    let anchor_x = if modifiers.alt {
        center.x
    } else if matches!(handle, Nw | W | Sw) {
        rect.right()
    } else {
        rect.x
    };
    let anchor_y = if modifiers.alt {
        center.y
    } else if matches!(handle, Nw | N | Ne) {
        rect.bottom()
    } else {
        rect.y
    };
    let factor = if modifiers.alt { 2. } else { 1. };
    let mut width = if moves_x {
        ((target.x - anchor_x).abs() * factor).clamp(*limits.start(), *limits.end())
    } else {
        rect.width
    };
    let mut height = if moves_y {
        ((target.y - anchor_y).abs() * factor).clamp(*limits.start(), *limits.end())
    } else {
        rect.height
    };
    if moves_x && moves_y {
        if modifiers.shift {
            width = width.max(height);
            height = width;
        } else if modifiers.ctrl && rect.width > 0. && rect.height > 0. {
            let scale = (width / rect.width).max(height / rect.height);
            width = rect.width * scale;
            height = rect.height * scale;
        }
    }
    let x = if modifiers.alt {
        center.x - width * 0.5
    } else if moves_x && target.x < anchor_x {
        anchor_x - width
    } else {
        anchor_x
    };
    let y = if modifiers.alt {
        center.y - height * 0.5
    } else if moves_y && target.y < anchor_y {
        anchor_y - height
    } else {
        anchor_y
    };
    Rect2::new(x, y, width, height)
}

fn resize_segment(
    start: [f32; 2],
    end: [f32; 2],
    handle: HandleKind,
    target: Point2,
    shift_snap: bool,
) -> ([f32; 2], [f32; 2]) {
    let (fixed, moving_start) = match handle {
        HandleKind::LineStart => (Point2::new(end[0], end[1]), Point2::new(start[0], start[1])),
        _ => (Point2::new(start[0], start[1]), Point2::new(end[0], end[1])),
    };
    let next = if shift_snap {
        snap_angle_15(fixed, target)
    } else {
        target
    };
    let len = (next.x - fixed.x).hypot(next.y - fixed.y);
    let final_pt = if len < MIN_LINE_LEN {
        moving_start
    } else {
        next
    };
    match handle {
        HandleKind::LineStart => ([final_pt.x, final_pt.y], end),
        _ => (start, [final_pt.x, final_pt.y]),
    }
}

fn resize_text(
    orig_pos: [f32; 2],
    content: &str,
    orig_size: f32,
    family: &str,
    handle: HandleKind,
    target: Point2,
) -> ([f32; 2], f32) {
    let orig_box = measure_text_rect(orig_pos, content, orig_size, family);
    let orig_w = orig_box.width.max(1.0);
    let orig_h = orig_box.height.max(1.0);

    let left = matches!(handle, HandleKind::Nw | HandleKind::Sw);
    let top = matches!(handle, HandleKind::Nw | HandleKind::Ne);
    let anchor = Point2::new(
        if left { orig_box.right() } else { orig_box.x },
        if top { orig_box.bottom() } else { orig_box.y },
    );
    let req_w = if left {
        anchor.x - target.x
    } else {
        target.x - anchor.x
    };
    let req_h = if top {
        anchor.y - target.y
    } else {
        target.y - anchor.y
    };
    let scale = ((req_w * orig_w + req_h * orig_h) / (orig_w * orig_w + orig_h * orig_h)).max(0.05);
    let next_size = (orig_size * scale).clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE);

    let rel = measure_text_rect([0.0, 0.0], content, next_size, family);
    let next_pos = match handle {
        HandleKind::Nw => [
            anchor.x - (rel.x + rel.width),
            anchor.y - (rel.y + rel.height),
        ],
        HandleKind::Ne => [anchor.x - rel.x, anchor.y - (rel.y + rel.height)],
        HandleKind::Sw => [anchor.x - (rel.x + rel.width), anchor.y - rel.y],
        _ => [anchor.x - rel.x, anchor.y - rel.y],
    };
    (next_pos, next_size)
}
