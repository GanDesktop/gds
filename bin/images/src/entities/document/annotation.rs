//! Signed-float annotation primitives, exact text bounds caching, and spatial transforms.

#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use gds::service::image::{
    PixelRect,
    ops::{RasterOp, measure_text_bounds_in_family},
};

use crate::shared::geometry::{Point2, Rect2, clip_segment_to_rect};

const TEXT_BOUNDS_CACHE_CAPACITY: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnnotationId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialTransformOp {
    RotateCw,
    RotateCcw,
    FlipHorizontal,
    FlipVertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub const RED: Self = Self::rgb(239, 68, 68);
    pub const AMBER: Self = Self::rgb(245, 158, 11);
    pub const GREEN: Self = Self::rgb(34, 197, 94);
    pub const BLUE: Self = Self::rgb(59, 130, 246);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const BLACK: Self = Self::rgb(17, 24, 39);

    pub const PALETTE: [Self; 6] = [
        Self::RED,
        Self::AMBER,
        Self::GREEN,
        Self::BLUE,
        Self::WHITE,
        Self::BLACK,
    ];

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn is_light(self) -> bool {
        299 * self.r as u32 + 587 * self.g as u32 + 114 * self.b as u32 > 145_000
    }

    pub const fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnnotationKind {
    Rect {
        rect: Rect2,
        stroke: RgbaColor,
        stroke_width: f32,
        fill: Option<RgbaColor>,
    },
    Ellipse {
        rect: Rect2,
        stroke: RgbaColor,
        stroke_width: f32,
        fill: Option<RgbaColor>,
    },
    Arrow {
        start: [f32; 2],
        end: [f32; 2],
        color: RgbaColor,
        stroke_width: f32,
    },
    Line {
        start: [f32; 2],
        end: [f32; 2],
        color: RgbaColor,
        stroke_width: f32,
    },
    Brush {
        points: Vec<[f32; 2]>,
        color: RgbaColor,
        stroke_width: f32,
    },
    Number {
        center: Point2,
        number: u32,
        radius: f32,
        color: RgbaColor,
    },
    Text {
        family: String,
        position: [f32; 2],
        content: String,
        color: RgbaColor,
        font_size: f32,
    },
    Mosaic {
        region: Rect2,
        block_size: u32,
    },
    GaussianBlur {
        region: Rect2,
        radius: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationItem {
    pub id: AnnotationId,
    pub kind: AnnotationKind,
}

/// Returns memoized SVG layout bounds for text at `position`, falling back to a minimum
/// clickable placeholder box for whitespace-only content.
pub fn measure_text_rect(position: [f32; 2], content: &str, font_size: f32, family: &str) -> Rect2 {
    if !position[0].is_finite() || !position[1].is_finite() {
        return Rect2::new(0.0, 0.0, 0.0, 0.0);
    }
    let size = if font_size.is_finite() {
        font_size.max(8.0)
    } else {
        8.0
    };
    if content.trim().is_empty() {
        let lines = content.split('\n').count().max(1) as f32;
        return Rect2::new(position[0], position[1], size, lines * size * 1.25);
    }

    static CACHE: OnceLock<Mutex<HashMap<(String, String, u32), [f32; 4]>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (content.to_owned(), family.to_owned(), size.to_bits());

    if let Some(&bounds) = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
    {
        return Rect2::new(
            position[0] + bounds[0],
            position[1] + bounds[1],
            bounds[2],
            bounds[3],
        );
    }

    let measured =
        measure_text_bounds_in_family([0.0, 0.0], content, size, family).unwrap_or_else(|| {
            let lines = content.split('\n').count().max(1) as f32;
            [0.0, 0.0, size, lines * size * 1.25]
        });

    {
        let mut guard = cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.len() >= TEXT_BOUNDS_CACHE_CAPACITY {
            guard.clear();
        }
        guard.insert(key, measured);
    }

    Rect2::new(
        position[0] + measured[0],
        position[1] + measured[1],
        measured[2],
        measured[3],
    )
}

impl AnnotationItem {
    pub fn to_raster_op(&self, canvas_w: u32, canvas_h: u32) -> Option<RasterOp> {
        match &self.kind {
            AnnotationKind::Rect {
                rect,
                stroke,
                stroke_width,
                fill,
            } => {
                let norm = rect.normalized();
                Some(RasterOp::Rect {
                    x: norm.x,
                    y: norm.y,
                    width: norm.width,
                    height: norm.height,
                    color: stroke.to_array(),
                    stroke_width: *stroke_width,
                    fill: fill.map(RgbaColor::to_array),
                })
            }
            AnnotationKind::Ellipse {
                rect,
                stroke,
                stroke_width,
                fill,
            } => {
                let norm = rect.normalized();
                Some(RasterOp::Ellipse {
                    x: norm.x,
                    y: norm.y,
                    width: norm.width,
                    height: norm.height,
                    color: stroke.to_array(),
                    stroke_width: *stroke_width,
                    fill: fill.map(RgbaColor::to_array),
                })
            }
            AnnotationKind::Arrow {
                start,
                end,
                color,
                stroke_width,
            } => Some(RasterOp::Arrow {
                start: *start,
                end: *end,
                color: color.to_array(),
                stroke_width: *stroke_width,
            }),
            AnnotationKind::Line {
                start,
                end,
                color,
                stroke_width,
            } => Some(RasterOp::Line {
                start: *start,
                end: *end,
                color: color.to_array(),
                stroke_width: *stroke_width,
            }),
            AnnotationKind::Brush {
                points,
                color,
                stroke_width,
            } => (!points.is_empty()).then(|| RasterOp::Polyline {
                points: points.clone(),
                color: color.to_array(),
                stroke_width: *stroke_width,
                opacity: 1.0,
            }),
            AnnotationKind::Number {
                center,
                number,
                radius,
                color,
            } => Some(RasterOp::StepBadge {
                center: [center.x, center.y],
                number: *number,
                radius: *radius,
                fill: color.to_array(),
                text_color: if color.is_light() {
                    [18, 18, 18, 255]
                } else {
                    [255; 4]
                },
            }),
            AnnotationKind::Text {
                family,
                position,
                content,
                color,
                font_size,
            } => Some(RasterOp::Text {
                position: *position,
                family: family.clone(),
                content: content.clone(),
                font_size: *font_size,
                color: color.to_array(),
            }),
            AnnotationKind::Mosaic { region, block_size } => {
                let clamped = region.to_clamped_pixel_rect(canvas_w, canvas_h)?;
                Some(RasterOp::Mosaic {
                    region: clamped,
                    block_size: *block_size,
                })
            }
            AnnotationKind::GaussianBlur { region, radius } => {
                let clamped = region.to_clamped_pixel_rect(canvas_w, canvas_h)?;
                Some(RasterOp::GaussianBlur {
                    region: clamped,
                    radius: *radius,
                })
            }
        }
    }

    /// Logical bounds used for selection outlines and resize handles.
    pub fn visual_bounds(&self) -> Rect2 {
        match &self.kind {
            AnnotationKind::Rect { rect, .. }
            | AnnotationKind::Ellipse { rect, .. }
            | AnnotationKind::Mosaic { region: rect, .. }
            | AnnotationKind::GaussianBlur { region: rect, .. } => rect.normalized(),
            AnnotationKind::Arrow { start, end, .. } | AnnotationKind::Line { start, end, .. } => {
                let x0 = start[0].min(end[0]);
                let y0 = start[1].min(end[1]);
                let x1 = start[0].max(end[0]);
                let y1 = start[1].max(end[1]);
                Rect2::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
            }
            AnnotationKind::Brush { points, .. } => {
                let Some(&first) = points.first() else {
                    return Rect2::new(0.0, 0.0, 0.0, 0.0);
                };
                let (mut min_x, mut min_y, mut max_x, mut max_y) =
                    (first[0], first[1], first[0], first[1]);
                for pt in &points[1..] {
                    min_x = min_x.min(pt[0]);
                    min_y = min_y.min(pt[1]);
                    max_x = max_x.max(pt[0]);
                    max_y = max_y.max(pt[1]);
                }
                Rect2::new(
                    min_x,
                    min_y,
                    (max_x - min_x).max(0.0),
                    (max_y - min_y).max(0.0),
                )
            }
            AnnotationKind::Number { center, radius, .. } => Rect2::new(
                center.x - radius,
                center.y - radius,
                2. * radius,
                2. * radius,
            ),
            AnnotationKind::Text {
                family,
                position,
                content,
                font_size,
                ..
            } => measure_text_rect(*position, content, *font_size, family),
        }
    }

    /// Conservative pixel-coverage bounds matching `ops::composite_ops_clipped` rasterization.
    pub fn dirty_bounds(&self) -> Rect2 {
        match &self.kind {
            AnnotationKind::Rect {
                rect, stroke_width, ..
            }
            | AnnotationKind::Ellipse {
                rect, stroke_width, ..
            } => {
                let pad = stroke_width.max(1.0) * 0.5 + 3.0;
                rect.normalized().expand(pad)
            }
            AnnotationKind::Line { stroke_width, .. }
            | AnnotationKind::Brush { stroke_width, .. } => {
                let pad = stroke_width.max(1.0) * 0.5 + 3.0;
                self.visual_bounds().expand(pad)
            }
            AnnotationKind::Arrow {
                start,
                end,
                stroke_width,
                ..
            } => {
                let sw = stroke_width.max(1.0);
                let len = (end[0] - start[0]).hypot(end[1] - start[1]);
                let head = (sw * 3.5).clamp(10.0, 32.0).min(len * 0.85);
                let wing = head * 0.55;
                let pad = wing.max(sw * 0.5) + 3.0;
                self.visual_bounds().expand(pad)
            }
            AnnotationKind::Text { .. } | AnnotationKind::Number { .. } => {
                self.visual_bounds().expand(5.0)
            }
            AnnotationKind::Mosaic { region, .. } | AnnotationKind::GaussianBlur { region, .. } => {
                region.normalized().expand(1.0)
            }
        }
    }

    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        let kind = match &self.kind {
            AnnotationKind::Rect {
                rect,
                stroke,
                stroke_width,
                fill,
            } => AnnotationKind::Rect {
                rect: Rect2::new(rect.x + dx, rect.y + dy, rect.width, rect.height),
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
                rect: Rect2::new(rect.x + dx, rect.y + dy, rect.width, rect.height),
                stroke: *stroke,
                stroke_width: *stroke_width,
                fill: *fill,
            },
            AnnotationKind::Arrow {
                start,
                end,
                color,
                stroke_width,
            } => AnnotationKind::Arrow {
                start: [start[0] + dx, start[1] + dy],
                end: [end[0] + dx, end[1] + dy],
                color: *color,
                stroke_width: *stroke_width,
            },
            AnnotationKind::Line {
                start,
                end,
                color,
                stroke_width,
            } => AnnotationKind::Line {
                start: [start[0] + dx, start[1] + dy],
                end: [end[0] + dx, end[1] + dy],
                color: *color,
                stroke_width: *stroke_width,
            },
            AnnotationKind::Brush {
                points,
                color,
                stroke_width,
            } => AnnotationKind::Brush {
                points: points.iter().map(|pt| [pt[0] + dx, pt[1] + dy]).collect(),
                color: *color,
                stroke_width: *stroke_width,
            },
            AnnotationKind::Number {
                center,
                number,
                radius,
                color,
            } => AnnotationKind::Number {
                center: Point2::new(center.x + dx, center.y + dy),
                number: *number,
                radius: *radius,
                color: *color,
            },
            AnnotationKind::Text {
                family,
                position,
                content,
                color,
                font_size,
            } => AnnotationKind::Text {
                position: [position[0] + dx, position[1] + dy],
                family: family.clone(),
                content: content.clone(),
                color: *color,
                font_size: *font_size,
            },
            AnnotationKind::Mosaic { region, block_size } => AnnotationKind::Mosaic {
                region: Rect2::new(region.x + dx, region.y + dy, region.width, region.height),
                block_size: *block_size,
            },
            AnnotationKind::GaussianBlur { region, radius } => AnnotationKind::GaussianBlur {
                region: Rect2::new(region.x + dx, region.y + dy, region.width, region.height),
                radius: *radius,
            },
        };
        Self { id: self.id, kind }
    }

    pub fn apply_spatial_transform(
        &self,
        op: SpatialTransformOp,
        canvas_w: u32,
        canvas_h: u32,
    ) -> Self {
        let wf = canvas_w as f32;
        let hf = canvas_h as f32;
        let kind = match &self.kind {
            AnnotationKind::Rect {
                rect,
                stroke,
                stroke_width,
                fill,
            } => AnnotationKind::Rect {
                rect: transform_rect(*rect, op, wf, hf),
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
                rect: transform_rect(*rect, op, wf, hf),
                stroke: *stroke,
                stroke_width: *stroke_width,
                fill: *fill,
            },
            AnnotationKind::Arrow {
                start,
                end,
                color,
                stroke_width,
            } => AnnotationKind::Arrow {
                start: transform_point(*start, op, wf, hf),
                end: transform_point(*end, op, wf, hf),
                color: *color,
                stroke_width: *stroke_width,
            },
            AnnotationKind::Line {
                start,
                end,
                color,
                stroke_width,
            } => AnnotationKind::Line {
                start: transform_point(*start, op, wf, hf),
                end: transform_point(*end, op, wf, hf),
                color: *color,
                stroke_width: *stroke_width,
            },
            AnnotationKind::Brush {
                points,
                color,
                stroke_width,
            } => AnnotationKind::Brush {
                points: points
                    .iter()
                    .map(|&pt| transform_point(pt, op, wf, hf))
                    .collect(),
                color: *color,
                stroke_width: *stroke_width,
            },
            AnnotationKind::Number {
                center,
                number,
                radius,
                color,
            } => {
                let [x, y] = transform_point([center.x, center.y], op, wf, hf);
                AnnotationKind::Number {
                    center: Point2::new(x, y),
                    number: *number,
                    radius: *radius,
                    color: *color,
                }
            }
            AnnotationKind::Text {
                family,
                position,
                content,
                color,
                font_size,
            } => {
                // Keep text upright while preserving exact group involutions via visual-center bijection.
                let bounds = measure_text_rect(*position, content, *font_size, family);
                let center = bounds.center();
                let next_center = transform_point([center.x, center.y], op, wf, hf);
                let next_position = [
                    position[0] + (next_center[0] - center.x),
                    position[1] + (next_center[1] - center.y),
                ];
                AnnotationKind::Text {
                    position: next_position,
                    family: family.clone(),
                    content: content.clone(),
                    color: *color,
                    font_size: *font_size,
                }
            }
            AnnotationKind::Mosaic { region, block_size } => AnnotationKind::Mosaic {
                region: transform_rect(*region, op, wf, hf),
                block_size: *block_size,
            },
            AnnotationKind::GaussianBlur { region, radius } => AnnotationKind::GaussianBlur {
                region: transform_rect(*region, op, wf, hf),
                radius: *radius,
            },
        };
        Self { id: self.id, kind }
    }

    pub fn apply_crop(&self, crop: PixelRect) -> Option<Self> {
        let translated = self.translated(-(crop.x as f32), -(crop.y as f32));
        let canvas_rect = Rect2::new(0.0, 0.0, crop.width as f32, crop.height as f32);
        let visible = match &translated.kind {
            AnnotationKind::Line {
                start,
                end,
                stroke_width,
                ..
            } => {
                let pad = stroke_width.max(1.0) * 0.5 + 2.0;
                clip_segment_to_rect(
                    Point2::new(start[0], start[1]),
                    Point2::new(end[0], end[1]),
                    canvas_rect.expand(pad),
                )
                .is_some()
            }
            AnnotationKind::Brush {
                points,
                stroke_width,
                ..
            } => {
                let pad = stroke_width.max(1.0) * 0.5 + 2.0;
                let expanded = canvas_rect.expand(pad);
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
            AnnotationKind::Arrow {
                start,
                end,
                stroke_width,
                ..
            } => {
                let sw = stroke_width.max(1.0);
                let len = (end[0] - start[0]).hypot(end[1] - start[1]);
                let head = (sw * 3.5).clamp(10.0, 32.0).min(len * 0.85);
                let pad = (head * 0.55).max(sw * 0.5) + 2.0;
                clip_segment_to_rect(
                    Point2::new(start[0], start[1]),
                    Point2::new(end[0], end[1]),
                    canvas_rect.expand(pad),
                )
                .is_some()
            }
            AnnotationKind::Mosaic { .. } | AnnotationKind::GaussianBlur { .. } => {
                translated.to_raster_op(crop.width, crop.height).is_some()
            }
            AnnotationKind::Rect { .. } | AnnotationKind::Ellipse { .. } => {
                translated.dirty_bounds().intersect(canvas_rect).is_some()
            }
            AnnotationKind::Number { .. } | AnnotationKind::Text { .. } => {
                translated.visual_bounds().intersect(canvas_rect).is_some()
            }
        };
        visible.then_some(translated)
    }

    pub const fn color(&self) -> Option<RgbaColor> {
        match &self.kind {
            AnnotationKind::Rect { stroke, .. } | AnnotationKind::Ellipse { stroke, .. } => {
                Some(*stroke)
            }
            AnnotationKind::Arrow { color, .. }
            | AnnotationKind::Line { color, .. }
            | AnnotationKind::Brush { color, .. }
            | AnnotationKind::Number { color, .. }
            | AnnotationKind::Text { color, .. } => Some(*color),
            AnnotationKind::Mosaic { .. } | AnnotationKind::GaussianBlur { .. } => None,
        }
    }

    pub fn with_color(&self, next_color: RgbaColor) -> Option<Self> {
        let mut updated = self.clone();
        match &mut updated.kind {
            AnnotationKind::Rect { stroke, .. } | AnnotationKind::Ellipse { stroke, .. } => {
                *stroke = next_color;
            }
            AnnotationKind::Arrow { color, .. }
            | AnnotationKind::Line { color, .. }
            | AnnotationKind::Brush { color, .. }
            | AnnotationKind::Number { color, .. }
            | AnnotationKind::Text { color, .. } => {
                *color = next_color;
            }
            AnnotationKind::Mosaic { .. } | AnnotationKind::GaussianBlur { .. } => return None,
        }
        Some(updated)
    }

    pub const fn stroke_width(&self) -> Option<f32> {
        match &self.kind {
            AnnotationKind::Rect { stroke_width, .. }
            | AnnotationKind::Ellipse { stroke_width, .. }
            | AnnotationKind::Arrow { stroke_width, .. }
            | AnnotationKind::Line { stroke_width, .. }
            | AnnotationKind::Brush { stroke_width, .. } => Some(*stroke_width),
            AnnotationKind::Number { .. }
            | AnnotationKind::Text { .. }
            | AnnotationKind::Mosaic { .. }
            | AnnotationKind::GaussianBlur { .. } => None,
        }
    }

    pub fn with_stroke_width(&self, next_width: f32) -> Option<Self> {
        let mut updated = self.clone();
        match &mut updated.kind {
            AnnotationKind::Rect { stroke_width, .. }
            | AnnotationKind::Ellipse { stroke_width, .. }
            | AnnotationKind::Arrow { stroke_width, .. }
            | AnnotationKind::Line { stroke_width, .. }
            | AnnotationKind::Brush { stroke_width, .. } => {
                *stroke_width = next_width;
            }
            AnnotationKind::Number { .. }
            | AnnotationKind::Text { .. }
            | AnnotationKind::Mosaic { .. }
            | AnnotationKind::GaussianBlur { .. } => return None,
        }
        Some(updated)
    }

    pub const fn marker_diameter(&self) -> Option<f32> {
        match self.kind {
            AnnotationKind::Number { radius, .. } => Some(radius * 2.),
            _ => None,
        }
    }

    pub fn with_marker_diameter(&self, diameter: f32) -> Option<Self> {
        let mut item = self.clone();
        let AnnotationKind::Number { radius, .. } = &mut item.kind else {
            return None;
        };
        *radius = diameter * 0.5;
        Some(item)
    }

    pub fn font_family(&self) -> Option<String> {
        match &self.kind {
            AnnotationKind::Text { family, .. } => Some(family.clone()),
            _ => None,
        }
    }

    pub fn with_font_family(&self, next: &str) -> Option<Self> {
        let mut item = self.clone();
        let AnnotationKind::Text { family, .. } = &mut item.kind else {
            return None;
        };
        *family = next.to_owned();
        Some(item)
    }
    pub const fn font_size(&self) -> Option<f32> {
        match &self.kind {
            AnnotationKind::Text { font_size, .. } => Some(*font_size),
            _ => None,
        }
    }

    pub fn with_font_size(&self, next_size: f32) -> Option<Self> {
        let mut updated = self.clone();
        match &mut updated.kind {
            AnnotationKind::Text { font_size, .. } => {
                *font_size = next_size;
                Some(updated)
            }
            _ => None,
        }
    }

    pub const fn mosaic_block_size(&self) -> Option<u32> {
        match &self.kind {
            AnnotationKind::Mosaic { block_size, .. } => Some(*block_size),
            _ => None,
        }
    }

    pub fn with_mosaic_block_size(&self, next_block_size: u32) -> Option<Self> {
        let mut updated = self.clone();
        match &mut updated.kind {
            AnnotationKind::Mosaic { block_size, .. } => {
                *block_size = next_block_size;
                Some(updated)
            }
            _ => None,
        }
    }

    pub const fn blur_radius(&self) -> Option<u32> {
        match &self.kind {
            AnnotationKind::GaussianBlur { radius, .. } => Some(*radius),
            _ => None,
        }
    }

    pub fn with_blur_radius(&self, next_radius: u32) -> Option<Self> {
        let mut updated = self.clone();
        match &mut updated.kind {
            AnnotationKind::GaussianBlur { radius, .. } => {
                *radius = next_radius;
                Some(updated)
            }
            _ => None,
        }
    }
}

fn transform_point(pt: [f32; 2], op: SpatialTransformOp, w: f32, h: f32) -> [f32; 2] {
    match op {
        SpatialTransformOp::RotateCw => [h - pt[1], pt[0]],
        SpatialTransformOp::RotateCcw => [pt[1], w - pt[0]],
        SpatialTransformOp::FlipHorizontal => [w - pt[0], pt[1]],
        SpatialTransformOp::FlipVertical => [pt[0], h - pt[1]],
    }
}

fn transform_rect(rect: Rect2, op: SpatialTransformOp, w: f32, h: f32) -> Rect2 {
    let n = rect.normalized();
    let p0 = transform_point([n.x, n.y], op, w, h);
    let p1 = transform_point([n.right(), n.bottom()], op, w, h);
    let x0 = p0[0].min(p1[0]);
    let y0 = p0[1].min(p1[1]);
    let x1 = p0[0].max(p1[0]);
    let y1 = p0[1].max(p1[1]);
    Rect2::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}
