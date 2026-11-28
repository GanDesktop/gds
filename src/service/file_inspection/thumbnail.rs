//! Local media thumbnails, independent of the UI and its image cache.
mod cache;
pub(super) mod external;
mod image;

use std::{path::Path, sync::Arc};

use super::{Error, Kind, Source};
pub use cache::{DEFAULT_MAX_THUMBNAIL_INVALIDATION_PATHS, ThumbnailInvalidation};
pub use external::{
    ExternalProbeReport, ExternalThumbnailers, SandboxError, SandboxMode, ThumbnailerInfo,
};
pub(super) use image::Generated;

pub const EDGE: u32 = 512;
#[cfg(test)]
const MAX_SOURCE_BYTES: u64 = super::DEFAULT_MAX_THUMBNAIL_SOURCE_MIB * 1024 * 1024;

/// Borrowed file identity passed into thumbnail cache and external thumbnailers.
#[derive(Clone, Copy)]
pub(super) struct Target<'a> {
    pub path: &'a Path,
    pub mime: Option<&'a str>,
    pub mime_ancestors: &'a [Arc<str>],
}

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
    pub(super) fn thumbnail_target(&self) -> Target<'_> {
        Target {
            path: &self.path,
            mime: self.mime.as_deref(),
            mime_ancestors: &self.mime_ancestors,
        }
    }

    /// Load a valid desktop thumbnail, or generate and cache a bounded PNG.
    ///
    /// # Errors
    /// Reports unreadable or unsupported media, image limits and thumbnail generation failures.
    #[cfg(test)]
    pub(super) fn thumbnail(&self) -> Result<Thumbnail, Error> {
        cache::load_with(
            self.thumbnail_target(),
            cache::Cache::for_user().as_ref(),
            || self.generate(),
        )
    }

    #[cfg(test)]
    pub(super) fn generate(&self) -> Result<Generated, Error> {
        self.generate_cancellable_with_options(
            &super::Options::default(),
            &tokio_util::sync::CancellationToken::new(),
        )
    }

    pub(super) fn generate_cancellable_with_options(
        &self,
        options: &super::Options,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<Generated, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Io(std::io::ErrorKind::Interrupted.into()));
        }
        let target = self.thumbnail_target();
        if let Some(external) = options.external_thumbnailers.as_deref()
            && external.has_user_override(self.mime.as_deref(), &self.mime_ancestors)
        {
            return external
                .generate(target, options.external_thumbnail_timeout, cancel, true)
                .map(Generated::image_only);
        }
        let builtin = match self.kind {
            Kind::Image => Some(image::load(&self.path, options.max_thumbnail_source_bytes)),
            Kind::Audio => Some((|| {
                let bytes = super::audio::cover(&self.path)?.ok_or(Error::NoCover)?;
                if cancel.is_cancelled() {
                    return Err(Error::Io(std::io::ErrorKind::Interrupted.into()));
                }
                let generated =
                    image::load_bytes(&bytes, None, options.max_thumbnail_source_bytes)?;
                Ok(Generated::image_only(generated.image))
            })()),
            _ => None,
        };
        match builtin {
            Some(Ok(img)) => Ok(img),
            Some(Err(err)) => {
                let can_fallback = (self.kind == Kind::Image
                    && matches!(err, Error::Image(_) | Error::Svg(_)))
                    || (self.kind == Kind::Audio && matches!(err, Error::Audio(_)));
                if can_fallback
                    && let Some(external) = options.external_thumbnailers.as_deref()
                    && external.has_match(self.mime.as_deref(), &self.mime_ancestors)
                {
                    return external
                        .generate(target, options.external_thumbnail_timeout, cancel, false)
                        .map(Generated::image_only);
                }
                Err(err)
            }
            None => {
                if let Some(external) = options.external_thumbnailers.as_deref()
                    && external.has_match(self.mime.as_deref(), &self.mime_ancestors)
                {
                    return external
                        .generate(target, options.external_thumbnail_timeout, cancel, false)
                        .map(Generated::image_only);
                }
                Err(Error::NoCover)
            }
        }
    }
}

/// Remove cached desktop thumbnails and failure markers for `path` (best effort).
pub fn invalidate_thumbnail_cache(path: &Path) {
    cache::invalidate_user_path(path);
}

/// # Errors
/// Reports thumbnail cache and generation failures.
pub(super) fn load_with(
    target: Target<'_>,
    generate: impl FnOnce() -> Result<Generated, Error>,
) -> Result<Thumbnail, Error> {
    cache::load_with(target, cache::Cache::for_user().as_ref(), generate)
}

/// Best-effort read-only lookup in a FreeDesktop shared `.sh_thumbnails/` repository.
pub(super) fn lookup_shared(target: Target<'_>) -> Option<Thumbnail> {
    cache::lookup_shared_for_target(target)
}

/// # Errors
/// Reports absent artwork and image decoding failures.
pub(super) fn cover(
    file: &lofty::file::TaggedFile,
    max_source_bytes: u64,
) -> Result<Generated, Error> {
    let picture = super::audio::picture(file).ok_or(Error::NoCover)?;
    let generated = image::load_bytes(picture.data(), None, max_source_bytes)?;
    Ok(Generated::image_only(generated.image))
}

#[cfg(test)]
mod tests;
