//! On-demand content metadata and thumbnails, independent of filesystem listings and UI.
mod audio;
mod error;
mod metadata;
mod thumbnail;

use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

pub use audio::{Error as AudioError, Tags as AudioTags};
pub use error::Error;
pub use metadata::{
    AudioFacts, AudioKind, DirectoryFacts, FileFacts, ImageFacts, ImageKind, LineEndings, Metadata,
    PixelSize, SymlinkFacts, TextEncoding, TextFacts, VideoFacts, VideoKind,
};
pub use thumbnail::{EDGE, Thumbnail};

pub const DEFAULT_MAX_THUMBNAIL_SOURCE_MIB: u64 = 32;
pub const DEFAULT_VIDEO_THUMBNAIL_TIMEOUT_SECS: u64 = 10;

/// Tunable bounds for on-demand thumbnail decoding and metadata inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Options {
    pub max_thumbnail_source_bytes: u64,
    pub video_thumbnail_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_thumbnail_source_bytes: DEFAULT_MAX_THUMBNAIL_SOURCE_MIB * 1024 * 1024,
            video_thumbnail_timeout: Duration::from_secs(DEFAULT_VIDEO_THUMBNAIL_TIMEOUT_SECS),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Kind {
    Image,
    Video,
    Audio,
    Text,
    Other,
    Directory,
    Symlink,
}

impl Kind {
    pub fn supports_thumbnail(self) -> bool {
        matches!(self, Self::Image | Self::Video | Self::Audio)
    }
}

/// Listing metadata identifies a version of the source for both cached fields.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Source {
    pub path: PathBuf,
    pub kind: Kind,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug)]
pub enum Request {
    Metadata,
    Thumbnail,
    Both,
}

impl Request {
    pub fn metadata(self) -> bool {
        matches!(self, Self::Metadata | Self::Both)
    }

    pub fn thumbnail(self) -> bool {
        matches!(self, Self::Thumbnail | Self::Both)
    }
}

/// Unavailable content (such as an absent cover) is distinct from a read failure.
#[derive(Default)]
pub enum Field<T> {
    #[default]
    NotRequested,
    Available(T),
    Unavailable,
    Failed(Error),
}

#[derive(Default)]
pub struct Inspection {
    pub metadata: Field<Metadata>,
    pub thumbnail: Field<Thumbnail>,
}

impl Source {
    /// Inspect only the requested content. Each field reports its own failure.
    /// Run on a background worker; no audio playback or UI allocation is involved.
    pub fn load(&self, request: Request) -> Inspection {
        self.load_cancellable(request, &tokio_util::sync::CancellationToken::new())
    }

    pub fn load_cancellable(
        &self,
        request: Request,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Inspection {
        self.load_cancellable_with_options(request, Options::default(), cancel)
    }

    pub fn load_cancellable_with_options(
        &self,
        request: Request,
        options: Options,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Inspection {
        if cancel.is_cancelled() {
            return Inspection::default();
        }
        let mut result = Inspection::default();
        if request.thumbnail() {
            result.thumbnail = if !self.kind.supports_thumbnail() {
                Field::Unavailable
            } else {
                let thumbnail = if self.kind == Kind::Audio && request.metadata() {
                    // The generator runs only on a disk-cache miss. Reuse its parse for
                    // metadata; on a cache hit the later metadata read skips artwork.
                    thumbnail::load_with(self, || {
                        let file = audio::read(&self.path, lofty::config::ParseOptions::new())?;
                        result.metadata = Field::Available(Metadata::File(FileFacts::Audio(
                            audio::Metadata::from_file(&file).into(),
                        )));
                        if cancel.is_cancelled() {
                            return Err(Error::Io(std::io::ErrorKind::Interrupted.into()));
                        }
                        thumbnail::cover(&file, options.max_thumbnail_source_bytes)
                    })
                } else {
                    thumbnail::load_with(self, || {
                        self.generate_cancellable_with_options(options, cancel)
                    })
                };
                match thumbnail {
                    Ok(thumbnail) => Field::Available(thumbnail),
                    Err(Error::NoCover) => Field::Unavailable,
                    Err(error) => Field::Failed(error),
                }
            };
        }
        if !cancel.is_cancelled()
            && request.metadata()
            && matches!(result.metadata, Field::NotRequested)
        {
            result.metadata = match self.metadata() {
                Ok(Metadata::File(FileFacts::Unknown)) => Field::Unavailable,
                Ok(metadata) => Field::Available(metadata),
                Err(error) => Field::Failed(error),
            };
        }
        result
    }

    /// # Errors
    /// Reports inaccessible sources or unreadable file headers.
    fn metadata(&self) -> Result<Metadata, Error> {
        match self.kind {
            Kind::Directory => return Ok(metadata::probe_directory(&self.path)?),
            Kind::Symlink => {
                return Ok(Metadata::Symlink(SymlinkFacts {
                    target: std::fs::read_link(&self.path)?,
                }));
            }
            Kind::Audio => match audio::Metadata::read(&self.path) {
                Ok(audio) => return Ok(Metadata::File(FileFacts::Audio(audio.into()))),
                Err(error) => {
                    tracing::debug!(path = %self.path.display(), %error, "audio metadata unavailable; using header probe")
                }
            },
            _ => {}
        }
        metadata::probe_path(
            &self.path,
            self.size,
            metadata::ProbeHint {
                text: self.kind == Kind::Text,
                audio: self.kind == Kind::Audio,
            },
        )
        .map_err(Error::from)
    }
}
