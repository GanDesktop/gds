//! Serialize composition on a worker. Transient gesture frames update incrementally via
//! [`GestureSession`]; committed frames guarantee bit-exactness with authoritative export pixels.

#[cfg(test)]
mod tests;

use super::*;
use crate::entities::document::{
    self, AnnotationKind, CompleteComposite, GesturePrepareInput, GestureSession, PreparedTextures,
    TransformState,
};
use gds::service::image::{self as engine, decode::render_svg_scaled, source::Format};
use image::RgbaImage;

/// Minimum zoom scale above 100% before unedited SVGs are re-rasterized at higher resolution.
const SVG_ZOOM_MIN_SCALE: f32 = 1.05;
/// Maximum edge length in pixels for zoomed SVG display rasterization.
const SVG_MAX_EDGE_PX: f32 = 8192.0;
/// Maximum pixel budget for zoomed SVG display rasterization (~64 MiB RGBA8).
const SVG_MAX_PIXELS: f32 = 16_777_216.0;
/// Scale quantization step for zoomed SVG rasterization.
const SVG_SCALE_QUANTUM: f32 = 0.05;
/// Relative scale difference within which an existing zoomed SVG rasterization is reused.
const SVG_SCALE_REUSE_TOLERANCE: f32 = 0.05;

struct Session {
    first: usize,
    gesture: GestureSession,
    items: Arc<[AnnotationItem]>,
}

pub(in crate::pages::workspace::ui) struct Compositor {
    source: Arc<RgbaImage>,
    transform: TransformState,
    svg_scale: Option<f32>,
    committed: PreparedTextures,
    items: Arc<[AnnotationItem]>,
    session: Option<Session>,
    // Tracks whether `committed.composited_rgba` is bit-identical to a single export pass.
    exact: bool,
}

struct Request {
    source: Arc<RgbaImage>,
    transform: TransformState,
    committed: Arc<[AnnotationItem]>,
    transient: Option<(usize, Arc<[AnnotationItem]>)>,
    svg: Option<(Arc<[u8]>, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameInstallCheck {
    doc_revision: u64,
    rendered_revision: u64,
    current_gesture: u64,
    rendered_gesture: u64,
    is_preview: bool,
    compositor_exact: bool,
    pointer_gesture_active: bool,
    editing_text: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameInstallDecision {
    cache_complete: bool,
    install_frame: bool,
}

fn decide_frame_install(check: FrameInstallCheck) -> FrameInstallDecision {
    let revision_matches = check.doc_revision == check.rendered_revision;
    let cache_complete = revision_matches && !check.is_preview && check.compositor_exact;
    let install_frame = revision_matches
        && check.current_gesture == check.rendered_gesture
        && (check.is_preview || (!check.pointer_gesture_active && !check.editing_text));
    FrameInstallDecision {
        cache_complete,
        install_frame,
    }
}

fn quantize_svg_scale(raw_zoom: f32) -> Option<f32> {
    if !raw_zoom.is_finite() || raw_zoom <= SVG_ZOOM_MIN_SCALE {
        return None;
    }
    let quantized = (raw_zoom / SVG_SCALE_QUANTUM).floor() * SVG_SCALE_QUANTUM;
    Some(quantized.max(SVG_ZOOM_MIN_SCALE))
}

fn svg_scale_matches(previous: Option<f32>, next: Option<f32>) -> bool {
    match (previous, next) {
        (None, None) => true,
        (Some(prev), Some(curr)) => (curr - prev).abs() / prev.max(1.0) < SVG_SCALE_REUSE_TOLERANCE,
        _ => false,
    }
}

fn is_cancelled(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<engine::Error>(),
        Some(engine::Error::Cancelled)
    )
}

impl Request {
    fn capture(state: &State) -> Option<Self> {
        let doc = state.doc.as_ref()?;
        let transient = state.tool.transient_annotations(doc);
        let svg = (doc.format() == Format::Svg && !doc.has_unbaked_ops() && transient.is_none())
            .then(|| {
                let (w, h) = doc.output_dimensions();
                if w == 0 || h == 0 {
                    return None;
                }
                let raw_zoom = state
                    .transform
                    .scale
                    .min(SVG_MAX_EDGE_PX / w.max(h) as f32)
                    .min((SVG_MAX_PIXELS / (w as f32 * h as f32)).sqrt());
                quantize_svg_scale(raw_zoom).map(|zoom| (doc.decoded.source_bytes.clone(), zoom))
            })
            .flatten();
        Some(Self {
            source: doc.decoded.rgba.clone(),
            transform: doc.transform,
            committed: Arc::from(doc.annotations.as_slice()),
            transient,
            svg,
        })
    }

    fn compose(
        self,
        previous: Option<Compositor>,
        cancel: &CancellationToken,
    ) -> Result<(Compositor, PreparedPayload), engine::Error> {
        let svg_eligible =
            self.svg.is_some() && self.transform.is_identity() && self.committed.is_empty();
        let scale = svg_eligible
            .then(|| self.svg.as_ref().map(|(_, zoom)| *zoom))
            .flatten();
        let mut compositor = match previous {
            Some(c)
                if Arc::ptr_eq(&self.source, &c.source)
                    && self.transform == c.transform
                    && svg_scale_matches(c.svg_scale, scale) =>
            {
                c
            }
            other => {
                let committed = if let Some((bytes, zoom)) = self.svg
                    && svg_eligible
                {
                    if cancel.is_cancelled() {
                        return Err(engine::Error::Cancelled);
                    }
                    let (w, h) = self.source.dimensions();
                    let scaled = render_svg_scaled(
                        &bytes,
                        (w as f32 * zoom).round().max(1.0) as u32,
                        (h as f32 * zoom).round().max(1.0) as u32,
                    )?;
                    let payload = document::display_tiles(&Arc::new(scaled), cancel)?;
                    PreparedTextures {
                        composited_rgba: self.source.clone(),
                        transformed_base: self.source.clone(),
                        payload,
                    }
                } else {
                    let cached = other
                        .filter(|c| Arc::ptr_eq(&self.source, &c.source))
                        .map(|c| (c.transform, c.committed.transformed_base));
                    document::build_render_textures(
                        self.source.clone(),
                        cached,
                        self.transform,
                        &self.committed,
                        cancel,
                    )?
                };
                Compositor {
                    source: self.source,
                    transform: self.transform,
                    svg_scale: scale,
                    committed,
                    items: self.committed.clone(),
                    session: None,
                    exact: true,
                }
            }
        };

        let display = if let Some((hint, items)) = self.transient {
            let first = hint.min(compositor.common_prefix(&items));
            compositor.step(first, &items, cancel)?
        } else if compositor.exact && compositor.kinds_match(&self.committed) {
            compositor.items = self.committed;
            compositor.session = None;
            compositor.committed.payload.clone()
        } else {
            let first = compositor.common_prefix(&self.committed);
            if compositor.can_commit_incrementally_exact(first, &self.committed) {
                let display = compositor.step(first, &self.committed, cancel)?;
                let session = compositor.session.take().expect("finished composite");
                compositor.committed.composited_rgba = session.gesture.into_pixels();
                compositor.committed.payload = display.clone();
                compositor.items = self.committed;
                compositor.exact = true;
                display
            } else {
                let rebuilt = document::build_render_textures(
                    compositor.source.clone(),
                    Some((
                        compositor.transform,
                        compositor.committed.transformed_base.clone(),
                    )),
                    self.transform,
                    &self.committed,
                    cancel,
                )?;
                let display = rebuilt.payload.clone();
                compositor.committed = rebuilt;
                compositor.items = self.committed;
                compositor.session = None;
                compositor.exact = true;
                display
            }
        };

        Ok((compositor, display))
    }
}

impl Compositor {
    fn common_prefix(&self, next: &[AnnotationItem]) -> usize {
        self.items
            .iter()
            .zip(next)
            .take_while(|(a, b)| a.kind == b.kind)
            .count()
    }

    fn kinds_match(&self, next: &[AnnotationItem]) -> bool {
        self.items.len() == next.len() && self.items.iter().zip(next).all(|(a, b)| a.kind == b.kind)
    }

    fn can_commit_incrementally_exact(&self, first: usize, next: &[AnnotationItem]) -> bool {
        if !self.exact {
            return false;
        }
        // A full pass from `transformed_base` never splits an SVG vector batch across passes.
        if first == 0 {
            return true;
        }
        // A topmost Gaussian blur flushes preceding ops before sampling the immutable prefix,
        // so an existing blur gesture session with matching radius stays bit-exact on commit.
        self.session.as_ref().is_some_and(|session| {
            session.first == first && Self::matching_topmost_blur(first, &session.items, next)
        })
    }

    fn matching_topmost_blur(
        first: usize,
        previous: &[AnnotationItem],
        next: &[AnnotationItem],
    ) -> bool {
        let blur_radius = |items: &[AnnotationItem]| match items.get(first..) {
            Some([item]) => match item.kind {
                AnnotationKind::GaussianBlur { radius, .. } => Some(radius),
                _ => None,
            },
            _ => None,
        };
        let Some(prev_radius) = blur_radius(previous) else {
            return false;
        };
        let Some(next_radius) = blur_radius(next) else {
            return false;
        };
        prev_radius == next_radius
            && previous
                .get(..first)
                .zip(next.get(..first))
                .is_some_and(|(a, b)| {
                    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.kind == y.kind)
                })
    }

    fn step(
        &mut self,
        first: usize,
        items: &[AnnotationItem],
        cancel: &CancellationToken,
    ) -> Result<PreparedPayload, engine::Error> {
        if self.session.as_ref().is_none_or(|s| s.first != first) {
            let gesture = GestureSession::prepare(
                GesturePrepareInput {
                    base_rgba: self.committed.transformed_base.clone(),
                    transform: TransformState::default(),
                    cached_base: None,
                    complete_rgba: Some(self.committed.composited_rgba.clone()),
                    display: Some(self.committed.payload.clone()),
                    initial_annotations: &self.items,
                    first_changed_idx: first,
                },
                cancel,
            )?;
            self.session = Some(Session {
                first,
                gesture,
                items: self.items.clone(),
            });
        }
        let session = self.session.as_mut().expect("prepared session");
        let payload = session.gesture.step(items, cancel)?;
        session.items = Arc::from(items);
        Ok(payload)
    }
}

impl Page {
    pub(in crate::pages::workspace::ui) fn render(self) {
        let w = self.window();
        if w.rendering.get() {
            w.queued.set(true);
            return;
        }
        let Some((request, revision, gesture, previous)) = self.change(|s| {
            Some((
                Request::capture(s)?,
                s.doc.as_ref()?.edit_revision,
                s.gesture,
                s.compositor.take(),
            ))
        }) else {
            return;
        };
        let preview = request.transient.as_ref().map(|(_, items)| items.clone());
        let epoch = w.epoch.get();
        let cancel = w.cancel.borrow().clone();
        w.rendering.set(true);
        self.background(
            "Composite image frame",
            move || Ok(request.compose(previous, &cancel)?),
            move |result| {
                w.rendering.set(false);
                if w.epoch.get() == epoch {
                    match result {
                        Ok((compositor, display)) => {
                            let applied = self.change(|s| {
                                let Some(doc) = &mut s.doc else {
                                    return false;
                                };
                                let decision = decide_frame_install(FrameInstallCheck {
                                    doc_revision: doc.edit_revision,
                                    rendered_revision: revision,
                                    current_gesture: s.gesture,
                                    rendered_gesture: gesture,
                                    is_preview: preview.is_some(),
                                    compositor_exact: compositor.exact,
                                    pointer_gesture_active: s.tool.is_pointer_gesture(),
                                    editing_text: s.tool.is_editing_text(),
                                });
                                if decision.cache_complete {
                                    doc.cache.complete = Some(CompleteComposite {
                                        revision,
                                        rgba: compositor.committed.composited_rgba.clone(),
                                    });
                                }
                                if decision.install_frame {
                                    s.frame = Some(display);
                                    s.preview_objects = preview;
                                }
                                // Mouse-up may arrive while the last drag is running. Keep its pixels
                                // and compare against the actual release position in the queued job.
                                s.compositor = Some(compositor);
                                decision.install_frame
                            });
                            if applied {
                                self.repaint();
                            }
                        }
                        Err(error) => {
                            if !is_cancelled(&error) {
                                self.error(gds::t_untracked!(
                                    "toast-render-failed",
                                    error = error.to_string()
                                ));
                            }
                        }
                    }
                }
                if w.queued.replace(false) {
                    self.render();
                }
            },
        );
    }
}
