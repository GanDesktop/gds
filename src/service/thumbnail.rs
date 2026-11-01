//! Local media thumbnails, independent of the UI and its image cache.
mod cache;
mod image;
mod video;

use std::{path::PathBuf, time::SystemTime};

/// Target edge of generated previews; cached images may use a larger size class.
pub const EDGE: u32 = 512;
const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Kind {
    Image,
    Video,
}

/// Listing metadata is part of the identity, so replacing a file invalidates its preview.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Source {
    pub path: PathBuf,
    pub kind: Kind,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}

/// An 8-bit RGBA PNG suitable for both desktop caches and UI image assets.
pub struct Thumbnail {
    pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read media: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot decode image: {0}")]
    Image(#[from] ::image::ImageError),
    #[error("cannot render SVG: {0}")]
    Svg(#[from] resvg::usvg::Error),
    #[error("cannot encode thumbnail PNG: {0}")]
    Png(#[from] png::EncodingError),
    #[error("thumbnail generation previously failed for this file")]
    CachedFailure,
    #[error("the image exceeds the preview size limit")]
    TooLarge,
    #[error("the preview source is not a regular file")]
    NotFile,
    #[error("video preview requires ffmpeg")]
    VideoUnavailable,
    #[error("video preview timed out")]
    VideoTimeout,
    #[error("cannot extract video frame: {0}")]
    Video(String),
}

impl Source {
    /// Load a valid desktop thumbnail, or generate and cache a bounded PNG.
    ///
    /// # Errors
    /// Reports unreadable or unsupported media, image limits and video extraction failures.
    pub fn load(&self) -> Result<Thumbnail, Error> {
        cache::load(self, cache::Cache::for_user().as_ref())
    }

    fn generate(&self) -> Result<::image::RgbaImage, Error> {
        match self.kind {
            Kind::Image => image::load(&self.path),
            Kind::Video => video::load(&self.path),
        }
    }
}

#[cfg(test)]
mod tests;
