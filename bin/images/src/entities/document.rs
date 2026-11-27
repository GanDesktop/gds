//! Document state, non-destructive spatial transforms, annotations, and zgui display cache.

mod annotation;
mod cache;
mod model;
mod transform;

pub use annotation::{
    AnnotationId, AnnotationItem, AnnotationKind, RgbaColor, SpatialTransformOp, measure_text_rect,
};
pub use cache::{
    CompleteComposite, GesturePrepareInput, GestureSession, PreparedPayload, PreparedTextures,
    build_render_textures, display_tiles,
};
pub use model::DocumentState;
pub use transform::TransformState;
