//! File opening prompt and zoom/fit navigation helpers.

use crate::shared::geometry::{FitMode, Size2, ViewportTransform};
const ZOOM_STEP: f32 = 1.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomPreset {
    ZoomIn,
    ZoomOut,
    Fit,
    Actual,
}

/// Applies a zoom/fit preset to `tf` centered around the current viewport.
pub fn apply_zoom_preset(
    preset: ZoomPreset,
    tf: &mut ViewportTransform,
    viewport: Size2,
    image: Size2,
) {
    match preset {
        ZoomPreset::ZoomIn => tf.set_scale_centered(tf.scale * ZOOM_STEP, viewport),
        ZoomPreset::ZoomOut => tf.set_scale_centered(tf.scale / ZOOM_STEP, viewport),
        ZoomPreset::Fit => {
            *tf = ViewportTransform::compute_fit(FitMode::FitToWindow, viewport, image)
        }
        ZoomPreset::Actual => {
            *tf = ViewportTransform::compute_fit(FitMode::ActualPixels, viewport, image);
        }
    }
}
