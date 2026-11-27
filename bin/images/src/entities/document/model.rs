//! Active loaded image document model combining base buffer, spatial edits, and annotations.

use std::sync::Arc;

use gds::service::image::{decode::DecodedImage, source::Format};
use image::RgbaImage;

use super::{
    annotation::AnnotationItem,
    cache::{CompleteComposite, RenderCache},
    transform::TransformState,
};

pub struct DocumentState {
    pub decoded: DecodedImage,
    pub transform: TransformState,
    pub annotations: Vec<AnnotationItem>,
    pub next_annotation_id: u64,
    /// Strictly monotonic revision bumped on every applied, undone, redone, or baked edit.
    pub edit_revision: u64,
    /// Revision at the last open or save clean point.
    pub saved_revision: u64,
    pub cache: RenderCache,
    saved_transform: TransformState,
    saved_annotations: Arc<[AnnotationItem]>,
}

impl DocumentState {
    pub fn from_decoded(decoded: DecodedImage) -> Self {
        Self {
            decoded,
            transform: TransformState::default(),
            annotations: Vec::new(),
            next_annotation_id: 1,
            edit_revision: 0,
            saved_revision: 0,
            cache: RenderCache::default(),
            saved_transform: TransformState::default(),
            saved_annotations: Arc::from([]),
        }
    }

    pub fn format(&self) -> Format {
        self.decoded.format
    }

    pub fn base_dimensions(&self) -> (u32, u32) {
        (self.decoded.rgba.width(), self.decoded.rgba.height())
    }

    pub fn output_dimensions(&self) -> (u32, u32) {
        let (w, h) = self.base_dimensions();
        self.transform.output_dimensions(w, h)
    }

    pub fn canvas_bounds(&self) -> crate::shared::geometry::Rect2 {
        let (width, height) = self.output_dimensions();
        crate::shared::geometry::Rect2::new(0., 0., width as f32, height as f32)
    }

    /// Whether there are non-destructive spatial transforms or vector/redaction annotations
    /// that still need to be baked when exporting pixels.
    pub fn has_unbaked_ops(&self) -> bool {
        !self.transform.is_identity() || !self.annotations.is_empty()
    }

    /// Whether the pixel state differs from the last opened or saved snapshot.
    pub fn is_dirty(&self) -> bool {
        self.edit_revision != self.saved_revision
            && (self.transform != self.saved_transform
                || self.annotations.as_slice() != self.saved_annotations.as_ref())
    }

    pub fn bump_revision(&mut self) {
        self.edit_revision = self.edit_revision.saturating_add(1);
    }

    pub fn allocate_annotation_id(&mut self) -> super::AnnotationId {
        let id = self.next_annotation_id;
        self.next_annotation_id += 1;
        super::AnnotationId(id)
    }

    pub fn mark_clean(&mut self) {
        self.saved_revision = self.edit_revision;
        self.saved_transform = self.transform;
        self.saved_annotations = Arc::from(self.annotations.as_slice());
    }

    /// Installs newly saved composited pixels as the clean base image without copying RGBA bytes.
    pub fn apply_saved_pixels(&mut self, composited: Arc<RgbaImage>) {
        self.decoded.rgba = Arc::clone(&composited);
        self.transform = TransformState::default();
        self.annotations.clear();
        self.bump_revision();
        self.cache.complete = Some(CompleteComposite {
            revision: self.edit_revision,
            rgba: composited,
        });
        self.mark_clean();
    }
}
