//! Isolated export cache (`CompleteComposite`), transient display frames (`PreparedPayload`),
//! and incremental prefix/dirty-region `GestureSession`.

#[cfg(test)]
mod tests;

use std::sync::Arc;

use gds::service::image::{
    Error, PixelRect,
    ops::{RasterOp, apply_gaussian_blur_from, apply_transform, composite_ops_clipped},
    tiles::{TileGrid, TileSpec},
};
use image::RgbaImage;
use tokio_util::sync::CancellationToken;

use super::{
    annotation::{AnnotationItem, AnnotationKind},
    transform::TransformState,
};
use crate::shared::geometry::Rect2;

/// Authoritative RGBA8 pixels composited exclusively from a committed [`super::DocumentState`].
///
/// Export and clipboard pipelines may reuse ONLY this buffer when `revision == doc.edit_revision`.
#[derive(Clone)]
pub struct CompleteComposite {
    pub revision: u64,
    pub rgba: Arc<RgbaImage>,
}

/// Immutable straight RGBA tiles shared with the window until replacement pixels are ready.
#[derive(Clone)]
pub struct PreparedPayload {
    pub tiles: Vec<(TileSpec, Arc<RgbaImage>)>,
    pub width: u32,
    pub height: u32,
}

pub struct PreparedTextures {
    pub composited_rgba: Arc<RgbaImage>,
    pub transformed_base: Arc<RgbaImage>,
    pub payload: PreparedPayload,
}

#[derive(Default)]
pub struct RenderCache {
    pub complete: Option<CompleteComposite>,
}

impl RenderCache {
    pub fn complete_at(&self, revision: u64) -> Option<&Arc<RgbaImage>> {
        self.complete
            .as_ref()
            .filter(|entry| entry.revision == revision)
            .map(|entry| &entry.rgba)
    }
}

/// Evaluates transforms + annotations on CPU and slices oversized canvases into Straight-RGBA8 tiles.
///
/// Reuses `base_rgba` directly via `Arc::clone` when no transform or annotation edits exist, and
/// reuses `cached_base` whenever `transform` matches.
///
/// # Errors
/// Returns [`Error::Cancelled`], [`Error::UnsupportedOperation`], [`Error::Svg`], or
/// [`Error::TooLarge`] if cancelled or rendering fails.
pub fn build_render_textures(
    base_rgba: Arc<RgbaImage>,
    cached_base: Option<(TransformState, Arc<RgbaImage>)>,
    transform: TransformState,
    annotations: &[AnnotationItem],
    cancel: &CancellationToken,
) -> Result<PreparedTextures, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let transformed_base = resolve_transformed_base(base_rgba, cached_base, transform)?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let composited_rgba = if annotations.is_empty() {
        Arc::clone(&transformed_base)
    } else {
        let mut composited = (*transformed_base).clone();
        let (cw, ch) = composited.dimensions();
        let ops: Vec<RasterOp> = annotations
            .iter()
            .filter_map(|item| item.to_raster_op(cw, ch))
            .collect();
        composite_ops_clipped(&mut composited, &ops, None)?;
        Arc::new(composited)
    };

    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let payload = display_tiles(&composited_rgba, cancel)?;
    Ok(PreparedTextures {
        composited_rgba,
        transformed_base,
        payload,
    })
}

// Local edits replace only intersecting GPU tiles, even when a photograph spans many.
const DISPLAY_TILE_EDGE: u32 = 512;

fn copy_tile(
    image: &RgbaImage,
    spec: TileSpec,
    cancel: &CancellationToken,
) -> Result<Arc<RgbaImage>, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let row_bytes = spec.width as usize * 4;
    let stride = image.width() as usize * 4;
    let mut pixels = Vec::with_capacity(row_bytes * spec.height as usize);
    for row in spec.y..spec.y + spec.height {
        let start = row as usize * stride + spec.x as usize * 4;
        pixels.extend_from_slice(&image.as_raw()[start..start + row_bytes]);
    }
    Ok(Arc::new(
        RgbaImage::from_raw(spec.width, spec.height, pixels).expect("packed tile rows"),
    ))
}

pub fn display_tiles(
    image: &Arc<RgbaImage>,
    cancel: &CancellationToken,
) -> Result<PreparedPayload, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let (width, height) = image.dimensions();
    let grid = TileGrid::compute(width, height, DISPLAY_TILE_EDGE);
    let mut tiles = Vec::with_capacity(grid.tiles.len());
    for spec in grid.tiles {
        let pixels = if (spec.width, spec.height) == (width, height) {
            image.clone()
        } else {
            copy_tile(image, spec, cancel)?
        };
        tiles.push((spec, pixels));
    }
    Ok(PreparedPayload {
        tiles,
        width,
        height,
    })
}

/// Inputs required to prepare a prefix-cached [`GestureSession`].
pub struct GesturePrepareInput<'a> {
    pub base_rgba: Arc<RgbaImage>,
    pub transform: TransformState,
    pub cached_base: Option<(TransformState, Arc<RgbaImage>)>,
    pub complete_rgba: Option<Arc<RgbaImage>>,
    pub display: Option<PreparedPayload>,
    pub initial_annotations: &'a [AnnotationItem],
    pub first_changed_idx: usize,
}

/// Pure-CPU incremental compositor for interactive drag/resize/text gestures.
///
/// Freezes `0..first_changed_idx` into `prefix_rgba` and composites only dirty regions
/// across `first_changed_idx..` in authoritative document stacking order.
pub struct GestureSession {
    first_changed_idx: usize,
    canvas_w: u32,
    canvas_h: u32,
    prefix_rgba: Arc<RgbaImage>,
    work_rgba: Arc<RgbaImage>,
    prev_annotations: Arc<[AnnotationItem]>,
    needs_full_first_pass: bool,
    display: Option<PreparedPayload>,
}

impl GestureSession {
    /// Freezes the immutable prefix `0..first_changed_idx` and initializes working buffers.
    ///
    /// # Errors
    /// Returns [`Error::Cancelled`], [`Error::UnsupportedOperation`], [`Error::Svg`], or
    /// [`Error::TooLarge`] if cancelled or prefix compositing fails.
    pub fn prepare(
        input: GesturePrepareInput<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let transformed_base =
            resolve_transformed_base(input.base_rgba, input.cached_base, input.transform)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let (canvas_w, canvas_h) = transformed_base.dimensions();
        let clamped_idx = input.first_changed_idx.min(input.initial_annotations.len());
        let valid_complete = input
            .complete_rgba
            .filter(|img| img.dimensions() == (canvas_w, canvas_h));

        let prefix_rgba = if clamped_idx == 0 {
            Arc::clone(&transformed_base)
        } else if clamped_idx == input.initial_annotations.len()
            && let Some(complete) = &valid_complete
        {
            Arc::clone(complete)
        } else {
            let mut buf = (*transformed_base).clone();
            let ops: Vec<RasterOp> = input.initial_annotations[..clamped_idx]
                .iter()
                .filter_map(|item| item.to_raster_op(canvas_w, canvas_h))
                .collect();
            composite_ops_clipped(&mut buf, &ops, None)?;
            Arc::new(buf)
        };

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let (work_rgba, needs_full_first_pass) = if let Some(complete) = valid_complete {
            (complete, false)
        } else if clamped_idx == input.initial_annotations.len() {
            (prefix_rgba.clone(), false)
        } else {
            (prefix_rgba.clone(), true)
        };

        Ok(Self {
            first_changed_idx: clamped_idx,
            canvas_w,
            canvas_h,
            prefix_rgba,
            work_rgba,
            prev_annotations: Arc::from(input.initial_annotations),
            needs_full_first_pass,
            display: input.display,
        })
    }

    /// Incrementally re-composites the minimal dirty region (including intersecting upper
    /// `Mosaic`/`GaussianBlur` regions) and returns an updated RGBA8 display payload.
    ///
    /// # Errors
    /// Returns [`Error::Cancelled`], [`Error::Svg`], or [`Error::TooLarge`].
    pub fn step(
        &mut self,
        next_annotations: &[AnnotationItem],
        cancel: &CancellationToken,
    ) -> Result<PreparedPayload, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let damage = match self.blur_change(next_annotations) {
            Some((before, after, radius)) if !self.needs_full_first_pass => {
                let removed = outside(before, after);
                let added = outside(after, before);
                if !removed.is_empty() || !added.is_empty() {
                    let work = Arc::make_mut(&mut self.work_rgba);
                    for rect in &removed {
                        copy_rgba_rect(&self.prefix_rgba, work, *rect);
                    }
                    for rect in &added {
                        if cancel.is_cancelled() {
                            return Err(Error::Cancelled);
                        }
                        apply_gaussian_blur_from(&self.prefix_rgba, work, *rect, radius);
                    }
                }
                removed.into_iter().chain(added).collect::<Vec<_>>()
            }
            _ => {
                let clip = if std::mem::take(&mut self.needs_full_first_pass) {
                    Some(PixelRect::new(0, 0, self.canvas_w, self.canvas_h))
                } else {
                    self.compute_dirty_clip(next_annotations)
                };
                if let Some(clip) = clip {
                    let work = Arc::make_mut(&mut self.work_rgba);
                    copy_rgba_rect(&self.prefix_rgba, work, clip);
                    let tail = next_annotations
                        .get(self.first_changed_idx..)
                        .unwrap_or(&[]);
                    let ops: Vec<_> = tail
                        .iter()
                        .filter_map(|item| item.to_raster_op(self.canvas_w, self.canvas_h))
                        .collect();
                    composite_ops_clipped(work, &ops, Some(clip))?;
                }
                clip.into_iter().collect()
            }
        };
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let display = match &mut self.display {
            Some(display) => {
                for (tile, pixels) in &mut display.tiles {
                    if damage.iter().any(|r| {
                        tile.x < r.right()
                            && tile.y < r.bottom()
                            && tile.x + tile.width > r.x
                            && tile.y + tile.height > r.y
                    }) {
                        *pixels = copy_tile(&self.work_rgba, *tile, cancel)?;
                    }
                }
                display
            }
            None => self.display.insert(display_tiles(&self.work_rgba, cancel)?),
        };
        self.prev_annotations = Arc::from(next_annotations);
        Ok(display.clone())
    }

    pub fn into_pixels(self) -> Arc<RgbaImage> {
        self.work_rgba
    }

    // A topmost blur samples an immutable prefix. Where the old and new bounds overlap,
    // its output is identical. A different radius or effects above it need the full compositor.
    fn blur_change(&self, next: &[AnnotationItem]) -> Option<(PixelRect, PixelRect, u32)> {
        let blur = |items: &[AnnotationItem]| {
            let [item] = items.get(self.first_changed_idx..)? else {
                return None;
            };
            let AnnotationKind::GaussianBlur { region, radius } = item.kind else {
                return None;
            };
            Some((
                region.to_clamped_pixel_rect(self.canvas_w, self.canvas_h)?,
                radius,
            ))
        };
        let (before, old_radius) = blur(&self.prev_annotations)?;
        let (after, radius) = blur(next)?;
        (radius == old_radius).then_some((before, after, radius))
    }

    fn compute_dirty_clip(&self, next_annotations: &[AnnotationItem]) -> Option<PixelRect> {
        let mut dirty: Option<Rect2> = None;
        let max_len = self.prev_annotations.len().max(next_annotations.len());
        for idx in self.first_changed_idx..max_len {
            let prev = self.prev_annotations.get(idx);
            let next = next_annotations.get(idx);
            // Temporary creation and duplication IDs may change at commit without changing pixels.
            if prev.map(|item| &item.kind) == next.map(|item| &item.kind) {
                continue;
            }
            if let Some(item) = prev {
                let bounds = item.dirty_bounds();
                dirty = Some(dirty.map_or(bounds, |acc| acc.union(bounds)));
            }
            if let Some(item) = next {
                let bounds = item.dirty_bounds();
                dirty = Some(dirty.map_or(bounds, |acc| acc.union(bounds)));
            }
        }

        let mut dirty = dirty?;
        let tail = next_annotations
            .get(self.first_changed_idx..)
            .unwrap_or(&[]);
        // Fixed-point propagation ensures upper pixel effects overlapping any dirty region
        // (including Blur halo neighborhoods) are fully recomputed without seams.
        loop {
            let mut expanded = dirty;
            for item in tail {
                match &item.kind {
                    AnnotationKind::Mosaic { region, .. } => {
                        let r = region.normalized();
                        if r.intersects_inclusive(expanded) {
                            expanded = expanded.union(r.expand(1.0));
                        }
                    }
                    AnnotationKind::GaussianBlur { region, radius } => {
                        let r = region.normalized();
                        let halo = ((*radius).clamp(1, 64) as f32) * 3.0 + 4.0;
                        if r.expand(halo).intersects_inclusive(expanded) {
                            expanded = expanded.union(r.expand(1.0));
                        }
                    }
                    _ => {}
                }
            }
            if expanded == dirty {
                break;
            }
            dirty = expanded;
        }

        dirty.to_clamped_pixel_rect(self.canvas_w, self.canvas_h)
    }
}

/// Disjoint strips of `rect` outside `other`, in integer pixel coordinates.
fn outside(rect: PixelRect, other: PixelRect) -> Vec<PixelRect> {
    let x = rect.x.max(other.x);
    let y = rect.y.max(other.y);
    let right = rect.right().min(other.right());
    let bottom = rect.bottom().min(other.bottom());
    if x >= right || y >= bottom {
        return vec![rect];
    }
    [
        PixelRect::new(rect.x, rect.y, rect.width, y - rect.y),
        PixelRect::new(rect.x, bottom, rect.width, rect.bottom() - bottom),
        PixelRect::new(rect.x, y, x - rect.x, bottom - y),
        PixelRect::new(right, y, rect.right() - right, bottom - y),
    ]
    .into_iter()
    .filter(|r| r.width > 0 && r.height > 0)
    .collect()
}

fn resolve_transformed_base(
    base_rgba: Arc<RgbaImage>,
    cached_base: Option<(TransformState, Arc<RgbaImage>)>,
    transform: TransformState,
) -> Result<Arc<RgbaImage>, Error> {
    if transform.is_identity() {
        return Ok(base_rgba);
    }
    if let Some((cached_tf, cached_img)) = cached_base
        && cached_tf == transform
    {
        return Ok(cached_img);
    }
    Ok(Arc::new(apply_transform(&base_rgba, &transform.spec)?))
}

fn copy_rgba_rect(src: &RgbaImage, dst: &mut RgbaImage, clip: PixelRect) {
    let Some(rect) = clip.clamp_to(dst.width().min(src.width()), dst.height().min(src.height()))
    else {
        return;
    };
    let src_stride = (src.width() as usize).saturating_mul(4);
    let dst_stride = (dst.width() as usize).saturating_mul(4);
    let x_byte = (rect.x as usize).saturating_mul(4);
    let row_bytes = (rect.width as usize).saturating_mul(4);
    let src_raw = src.as_raw();
    let dst_raw: &mut [u8] = dst.as_mut();

    for row in rect.y as usize..rect.bottom() as usize {
        let s = row * src_stride + x_byte;
        let d = row * dst_stride + x_byte;
        dst_raw[d..d + row_bytes].copy_from_slice(&src_raw[s..s + row_bytes]);
    }
}
