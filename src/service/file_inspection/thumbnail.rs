//! Local media thumbnails, independent of the UI and its image cache.
mod cache;
mod image;
mod video;

use super::{Error, Kind, Source};

pub const EDGE: u32 = 512;
const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;

/// An 8-bit RGBA PNG suitable for both desktop caches and UI image assets.
pub struct Thumbnail {
    pub bytes: Vec<u8>,
}

impl Source {
    /// Load a valid desktop thumbnail, or generate and cache a bounded PNG.
    ///
    /// # Errors
    /// Reports unreadable or unsupported media, image limits and video extraction failures.
    pub(super) fn thumbnail(&self) -> Result<Thumbnail, Error> {
        cache::load(self, cache::Cache::for_user().as_ref())
    }

    pub(super) fn generate(&self) -> Result<::image::RgbaImage, Error> {
        match self.kind {
            Kind::Image => image::load(&self.path),
            Kind::Video => video::load(&self.path),
            Kind::Audio => {
                let bytes = super::audio::cover(&self.path)?.ok_or(Error::NoCover)?;
                image::load_bytes(&bytes, None)
            }
            _ => Err(Error::NoCover),
        }
    }
}

/// # Errors
/// Reports thumbnail cache and generation failures.
pub(super) fn load_with(
    source: &Source,
    generate: impl FnOnce() -> Result<::image::RgbaImage, Error>,
) -> Result<Thumbnail, Error> {
    cache::load_with(source, cache::Cache::for_user().as_ref(), generate)
}

/// # Errors
/// Reports absent artwork and image decoding failures.
pub(super) fn cover(file: &lofty::file::TaggedFile) -> Result<::image::RgbaImage, Error> {
    let picture = super::audio::picture(file).ok_or(Error::NoCover)?;
    image::load_bytes(picture.data(), None)
}

#[cfg(test)]
mod tests;
