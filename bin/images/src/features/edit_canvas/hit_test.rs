//! Screen-space tolerance hit-testing, tool-aware filtering, Alt-cycle stack, and marquee hits.

use crate::{
    entities::document::{AnnotationId, AnnotationItem, AnnotationKind},
    shared::geometry::{MIN_ZOOM, Point2, Rect2, clip_segment_to_rect},
};

use super::{selection::SelectionSet, tool_fsm::ToolMode};

const HANDLE_HIT_RADIUS_PX: f32 = 7.0;
const STROKE_PAD_PX: f32 = 6.0;
const TEXT_PAD_PX: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKind {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
    LineStart,
    LineEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitTarget {
    Handle {
        id: AnnotationId,
        handle: HandleKind,
    },
    Body {
        id: AnnotationId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverTarget {
    None,
    Handle {
        id: AnnotationId,
        handle: HandleKind,
    },
    Body {
        id: AnnotationId,
        discover_redaction: bool,
    },
}

pub fn handles_for_item(item: &AnnotationItem) -> Vec<(HandleKind, Point2)> {
    let box_handles = |rect: Rect2| {
        let n = rect.normalized();
        let cx = n.x + n.width * 0.5;
        let cy = n.y + n.height * 0.5;
        vec![
            (HandleKind::Nw, Point2::new(n.x, n.y)),
            (HandleKind::N, Point2::new(cx, n.y)),
            (HandleKind::Ne, Point2::new(n.right(), n.y)),
            (HandleKind::E, Point2::new(n.right(), cy)),
            (HandleKind::Se, Point2::new(n.right(), n.bottom())),
            (HandleKind::S, Point2::new(cx, n.bottom())),
            (HandleKind::Sw, Point2::new(n.x, n.bottom())),
            (HandleKind::W, Point2::new(n.x, cy)),
        ]
    };
    match &item.kind {
        AnnotationKind::Rect { rect, .. }
        | AnnotationKind::Ellipse { rect, .. }
        | AnnotationKind::Mosaic { region: rect, .. }
        | AnnotationKind::GaussianBlur { region: rect, .. } => box_handles(*rect),
        AnnotationKind::Brush { .. } => box_handles(item.visual_bounds()),
        AnnotationKind::Arrow { start, end, .. } | AnnotationKind::Line { start, end, .. } => {
            vec![
                (HandleKind::LineStart, Point2::new(start[0], start[1])),
                (HandleKind::LineEnd, Point2::new(end[0], end[1])),
            ]
        }
        AnnotationKind::Number { .. } | AnnotationKind::Text { .. } => {
            let b = item.visual_bounds();
            vec![
                (HandleKind::Nw, Point2::new(b.x, b.y)),
                (HandleKind::Ne, Point2::new(b.right(), b.y)),
                (HandleKind::Se, Point2::new(b.right(), b.bottom())),
                (HandleKind::Sw, Point2::new(b.x, b.bottom())),
            ]
        }
    }
}

pub fn hit_test_canvas(
    annotations: &[AnnotationItem],
    selection: &SelectionSet,
    img_pt: Point2,
    tool: ToolMode,
    scale: f32,
) -> Option<HitTarget> {
    if tool != ToolMode::Pointer {
        return None;
    }
    let inv_scale = 1.0 / scale.max(MIN_ZOOM);
    let handle_radius = HANDLE_HIT_RADIUS_PX * inv_scale;

    if selection.len() == 1
        && let Some(primary_id) = selection.primary()
        && let Some(item) = annotations.iter().find(|entry| entry.id == primary_id)
        && let Some(handle) = hit_test_handles(item, img_pt, handle_radius)
    {
        return Some(HitTarget::Handle {
            id: primary_id,
            handle,
        });
    }

    let stack = hit_test_cycle_stack(annotations, img_pt, scale);
    // Keep an already-selected item draggable even when a higher unselected item overlaps it.
    stack
        .iter()
        .copied()
        .find(|id| selection.contains(*id))
        .or_else(|| stack.first().copied())
        .map(|id| HitTarget::Body { id })
}

pub fn hover_test_canvas(
    annotations: &[AnnotationItem],
    selection: &SelectionSet,
    img_pt: Point2,
    tool: ToolMode,
    scale: f32,
) -> HoverTarget {
    match hit_test_canvas(annotations, selection, img_pt, tool, scale) {
        Some(HitTarget::Handle { id, handle }) => HoverTarget::Handle { id, handle },
        Some(HitTarget::Body { id }) => {
            let discover_redaction = !selection.contains(id)
                && annotations.iter().any(|item| {
                    item.id == id
                        && matches!(
                            item.kind,
                            AnnotationKind::Mosaic { .. } | AnnotationKind::GaussianBlur { .. }
                        )
                });
            HoverTarget::Body {
                id,
                discover_redaction,
            }
        }
        None => HoverTarget::None,
    }
}

pub fn hit_test_cycle_stack(
    annotations: &[AnnotationItem],
    img_pt: Point2,
    scale: f32,
) -> Vec<AnnotationId> {
    let inv_scale = 1.0 / scale.max(MIN_ZOOM);
    let stroke_pad = STROKE_PAD_PX * inv_scale;
    let text_pad = TEXT_PAD_PX * inv_scale;

    annotations
        .iter()
        .rev()
        .filter(|item| hit_test_body(item, img_pt, stroke_pad, text_pad))
        .map(|item| item.id)
        .collect()
}

pub fn marquee_hits(annotations: &[AnnotationItem], marquee: Rect2) -> Vec<AnnotationId> {
    let norm = marquee.normalized();
    annotations
        .iter()
        .filter(|item| match &item.kind {
            AnnotationKind::Line {
                start,
                end,
                stroke_width,
                ..
            }
            | AnnotationKind::Arrow {
                start,
                end,
                stroke_width,
                ..
            } => {
                let pad = stroke_width.max(1.0) * 0.5;
                clip_segment_to_rect(
                    Point2::new(start[0], start[1]),
                    Point2::new(end[0], end[1]),
                    norm.expand(pad),
                )
                .is_some()
            }
            AnnotationKind::Brush {
                points,
                stroke_width,
                ..
            } => {
                let expanded = norm.expand(stroke_width.max(1.0) * 0.5);
                match points.as_slice() {
                    [] => false,
                    [only] => expanded.contains(Point2::new(only[0], only[1])),
                    many => many.windows(2).any(|seg| {
                        clip_segment_to_rect(
                            Point2::new(seg[0][0], seg[0][1]),
                            Point2::new(seg[1][0], seg[1][1]),
                            expanded,
                        )
                        .is_some()
                    }),
                }
            }
            _ => item.visual_bounds().intersects_inclusive(norm),
        })
        .map(|item| item.id)
        .collect()
}

fn hit_test_handles(item: &AnnotationItem, pt: Point2, radius: f32) -> Option<HandleKind> {
    let r_sq = radius * radius;
    let mut best: Option<(HandleKind, f32)> = None;
    for (kind, pos) in handles_for_item(item) {
        let dx = pt.x - pos.x;
        let dy = pt.y - pos.y;
        let dist_sq = dx * dx + dy * dy;
        if dist_sq <= r_sq && best.is_none_or(|(_, best_sq)| dist_sq < best_sq) {
            best = Some((kind, dist_sq));
        }
    }
    best.map(|(kind, _)| kind)
}

fn hit_test_body(item: &AnnotationItem, pt: Point2, stroke_pad: f32, text_pad: f32) -> bool {
    match &item.kind {
        AnnotationKind::Rect {
            rect,
            stroke_width,
            fill,
            ..
        } => {
            let n = rect.normalized();
            let pad = stroke_width.max(1.0) * 0.5 + stroke_pad;
            let hit_interior = fill.is_some();
            if hit_interior {
                n.expand(pad).contains(pt)
            } else {
                min_dist_to_rect_perimeter(pt, n) <= pad
            }
        }
        AnnotationKind::Ellipse {
            rect,
            stroke_width,
            fill,
            ..
        } => {
            let n = rect.normalized();
            let pad = stroke_width.max(1.0) * 0.5 + stroke_pad;
            let hit_interior = fill.is_some();
            ellipse_hit(pt, n, pad, hit_interior)
        }
        AnnotationKind::Line {
            start,
            end,
            stroke_width,
            ..
        } => {
            let pad = stroke_width.max(1.0) * 0.5 + stroke_pad;
            point_to_segment_distance(
                pt,
                Point2::new(start[0], start[1]),
                Point2::new(end[0], end[1]),
            ) <= pad
        }
        AnnotationKind::Arrow {
            start,
            end,
            stroke_width,
            ..
        } => arrow_hit(pt, *start, *end, *stroke_width, stroke_pad),
        AnnotationKind::Brush {
            points,
            stroke_width,
            ..
        } => {
            let pad = stroke_width.max(1.0) * 0.5 + stroke_pad;
            match points.as_slice() {
                [] => false,
                [only] => (pt.x - only[0]).hypot(pt.y - only[1]) <= pad,
                many => many.windows(2).any(|seg| {
                    point_to_segment_distance(
                        pt,
                        Point2::new(seg[0][0], seg[0][1]),
                        Point2::new(seg[1][0], seg[1][1]),
                    ) <= pad
                }),
            }
        }
        AnnotationKind::Number { center, radius, .. } => {
            (pt.x - center.x).hypot(pt.y - center.y) <= radius + stroke_pad
        }
        AnnotationKind::Text { .. } => item.visual_bounds().expand(text_pad).contains(pt),
        AnnotationKind::Mosaic { region, .. } | AnnotationKind::GaussianBlur { region, .. } => {
            region.normalized().expand(stroke_pad).contains(pt)
        }
    }
}

fn min_dist_to_rect_perimeter(pt: Point2, r: Rect2) -> f32 {
    let tl = Point2::new(r.x, r.y);
    let tr = Point2::new(r.right(), r.y);
    let br = Point2::new(r.right(), r.bottom());
    let bl = Point2::new(r.x, r.bottom());
    point_to_segment_distance(pt, tl, tr)
        .min(point_to_segment_distance(pt, tr, br))
        .min(point_to_segment_distance(pt, br, bl))
        .min(point_to_segment_distance(pt, bl, tl))
}

fn ellipse_hit(pt: Point2, n: Rect2, pad: f32, include_interior: bool) -> bool {
    let cx = n.x + n.width * 0.5;
    let cy = n.y + n.height * 0.5;
    let rx = (n.width * 0.5).max(0.5);
    let ry = (n.height * 0.5).max(0.5);
    let dx = pt.x - cx;
    let dy = pt.y - cy;

    let ox = rx + pad;
    let oy = ry + pad;
    let outer = (dx * dx) / (ox * ox) + (dy * dy) / (oy * oy);
    if outer > 1.0 {
        return false;
    }
    if include_interior || rx <= pad || ry <= pad {
        return true;
    }
    let ix = rx - pad;
    let iy = ry - pad;
    let inner = (dx * dx) / (ix * ix) + (dy * dy) / (iy * iy);
    inner >= 1.0
}

fn arrow_hit(
    pt: Point2,
    start: [f32; 2],
    end: [f32; 2],
    stroke_width: f32,
    stroke_pad: f32,
) -> bool {
    let p0 = Point2::new(start[0], start[1]);
    let p1 = Point2::new(end[0], end[1]);
    let sw = stroke_width.max(1.0);
    if point_to_segment_distance(pt, p0, p1) <= sw * 0.5 + stroke_pad {
        return true;
    }
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let len = dx.hypot(dy);
    if !len.is_finite() || len < 1e-3 {
        return false;
    }
    let ux = dx / len;
    let uy = dy / len;
    let head = (sw * 3.5).clamp(10.0, 32.0).min(len * 0.85);
    let wing = head * 0.55;
    let bx = end[0] - ux * head;
    let by = end[1] - uy * head;
    let w1 = Point2::new(bx - uy * wing, by + ux * wing);
    let w2 = Point2::new(bx + uy * wing, by - ux * wing);
    point_in_triangle(pt, p1, w1, w2)
        || point_to_segment_distance(pt, p1, w1) <= stroke_pad
        || point_to_segment_distance(pt, p1, w2) <= stroke_pad
        || point_to_segment_distance(pt, w1, w2) <= stroke_pad
}

fn point_in_triangle(p: Point2, a: Point2, b: Point2, c: Point2) -> bool {
    let d1 = cross_sign(p, a, b);
    let d2 = cross_sign(p, b, c);
    let d3 = cross_sign(p, c, a);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

fn cross_sign(p1: Point2, p2: Point2, p3: Point2) -> f32 {
    (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y)
}

fn point_to_segment_distance(pt: Point2, a: Point2, b: Point2) -> f32 {
    let vx = b.x - a.x;
    let vy = b.y - a.y;
    let len_sq = vx * vx + vy * vy;
    if len_sq <= 1e-6 {
        return (pt.x - a.x).hypot(pt.y - a.y);
    }
    let t = (((pt.x - a.x) * vx + (pt.y - a.y) * vy) / len_sq).clamp(0.0, 1.0);
    let proj_x = a.x + t * vx;
    let proj_y = a.y + t * vy;
    (pt.x - proj_x).hypot(pt.y - proj_y)
}
