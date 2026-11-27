//! Pure media decoding, processing, metadata inspection/rewriting, and atomic image I/O.
//!
//! Codecs, color management, tile slicing and raster composition run independently of the UI.

pub mod color;
pub mod decode;
pub mod encode;
pub mod io;
pub mod metadata;
pub mod ops;
pub mod source;
mod svgz;
pub mod tiles;

use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

/// Domain errors returned by the backend image engine and atomic file writer.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("image I/O error{}: {source}", path.as_ref().map(|p| format!(" at {}", p.display())).unwrap_or_default())]
    Io {
        path: Option<PathBuf>,
        #[source]
        source: std::io::Error,
    },
    #[error("image codec error: {0}")]
    Decode(#[from] image::ImageError),
    #[error("SVG rendering error: {0}")]
    Svg(#[from] resvg::usvg::Error),
    #[error("PNG encoding error: {0}")]
    PngEncode(#[from] png::EncodingError),
    #[error("color profile transform failed: {0}")]
    ColorTransform(String),
    #[error("unsupported image format: {0}")]
    UnsupportedFormat(String),
    #[error("unsupported operation: {0}")]
    UnsupportedOperation(&'static str),
    #[error("image size ({actual_bytes} bytes) exceeds limit ({limit_bytes} bytes)")]
    TooLarge { limit_bytes: u64, actual_bytes: u64 },
    #[error("image dimensions {width}x{height} exceed maximum edge {max_edge}")]
    DimensionsTooLarge {
        width: u32,
        height: u32,
        max_edge: u32,
    },
    #[error("invalid metadata payload: {0}")]
    InvalidMetadata(&'static str),
    #[error("image changed externally before save: {0}")]
    ExternalConflict(PathBuf),
    #[error("refusing to overwrite symbolic link: {0}")]
    Symlink(PathBuf),
    #[error("destination is read-only: {0}")]
    ReadOnly(PathBuf),
    #[error("image task cancelled")]
    Cancelled,
}

impl Error {
    pub(crate) fn io(path: Option<PathBuf>, source: std::io::Error) -> Self {
        Self::Io { path, source }
    }
}

/// Unsigned integer rectangle in decoded document or canvas pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    pub const fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    /// Clamps this rectangle to `0..bounds_w` × `0..bounds_h`, returning `None` if empty.
    pub fn clamp_to(self, bounds_w: u32, bounds_h: u32) -> Option<Self> {
        let x0 = self.x.min(bounds_w);
        let y0 = self.y.min(bounds_h);
        let x1 = self.right().min(bounds_w);
        let y1 = self.bottom().min(bounds_h);
        let width = x1.saturating_sub(x0);
        let height = y1.saturating_sub(y0);
        (width > 0 && height > 0).then_some(Self::new(x0, y0, width, height))
    }

    /// Expands outward by `margin` on each side and clamps to image bounds.
    pub fn expand_clamped(self, margin: u32, bounds_w: u32, bounds_h: u32) -> Option<Self> {
        let clamped = self.clamp_to(bounds_w, bounds_h)?;
        let x0 = clamped.x.saturating_sub(margin);
        let y0 = clamped.y.saturating_sub(margin);
        let x1 = clamped.right().saturating_add(margin).min(bounds_w);
        let y1 = clamped.bottom().saturating_add(margin).min(bounds_h);
        Some(Self::new(
            x0,
            y0,
            x1.saturating_sub(x0),
            y1.saturating_sub(y0),
        ))
    }
}

pub(crate) fn system_fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            // Fontconfig aliases can designate a family that is absent on the desktop.
            // usvg drops text runs when the generic sans-serif query has no match.
            use resvg::usvg::fontdb::{Family, Query};
            if db
                .query(&Query {
                    families: &[Family::SansSerif],
                    ..Default::default()
                })
                .is_none()
            {
                let preferred = ["Noto Sans", "DejaVu Sans", "Liberation Sans"];
                let fallback = preferred
                    .iter()
                    .find_map(|name| {
                        db.query(&Query {
                            families: &[Family::Name(name)],
                            ..Default::default()
                        })
                        .map(|_| name.to_string())
                    })
                    .or_else(|| {
                        db.faces()
                            .find_map(|face| face.families.first().map(|(name, _)| name.clone()))
                    });
                if let Some(name) = fallback {
                    db.set_sans_serif_family(name)
                }
            }
            Arc::new(db)
        })
        .clone()
}
