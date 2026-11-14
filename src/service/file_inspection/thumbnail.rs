//! Local media thumbnails, independent of the UI and its image cache.
mod cache;
mod image;
mod video;

use super::{Error, Kind, Source};

pub const EDGE: u32 = 512;
#[cfg(test)]
const MAX_SOURCE_BYTES: u64 = super::DEFAULT_MAX_THUMBNAIL_SOURCE_MIB * 1024 * 1024;

/// A decoded 8-bit RGBA thumbnail image ready for UI rendering and desktop caching.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thumbnail {
    image: ::image::RgbaImage,
}

impl Thumbnail {
    pub(super) fn new(image: ::image::RgbaImage) -> Self {
        Self { image }
    }

    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }

    pub fn as_rgba(&self) -> &::image::RgbaImage {
        &self.image
    }

    pub fn into_rgba(self) -> ::image::RgbaImage {
        self.image
    }
}

impl Source {
    /// Load a valid desktop thumbnail, or generate and cache a bounded PNG.
    ///
    /// # Errors
    /// Reports unreadable or unsupported media, image limits and video extraction failures.
    #[cfg(test)]
    pub(super) fn thumbnail(&self) -> Result<Thumbnail, Error> {
        cache::load(self, cache::Cache::for_user().as_ref())
    }

    #[cfg(test)]
    pub(super) fn generate(&self) -> Result<::image::RgbaImage, Error> {
        self.generate_cancellable_with_options(
            super::Options::default(),
            &tokio_util::sync::CancellationToken::new(),
        )
    }

    pub(super) fn generate_cancellable_with_options(
        &self,
        options: super::Options,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<::image::RgbaImage, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Io(std::io::ErrorKind::Interrupted.into()));
        }
        match self.kind {
            Kind::Image => image::load(&self.path, options.max_thumbnail_source_bytes),
            Kind::Video => video::load(&self.path, options.video_thumbnail_timeout, cancel),
            Kind::Audio => {
                let bytes = super::audio::cover(&self.path)?.ok_or(Error::NoCover)?;
                if cancel.is_cancelled() {
                    return Err(Error::Io(std::io::ErrorKind::Interrupted.into()));
                }
                image::load_bytes(&bytes, None, options.max_thumbnail_source_bytes)
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
pub(super) fn cover(
    file: &lofty::file::TaggedFile,
    max_source_bytes: u64,
) -> Result<::image::RgbaImage, Error> {
    let picture = super::audio::picture(file).ok_or(Error::NoCover)?;
    image::load_bytes(picture.data(), None, max_source_bytes)
}

#[cfg(test)]
mod tests;
