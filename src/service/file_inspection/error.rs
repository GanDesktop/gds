#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read file: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot decode image: {0}")]
    Image(#[source] ::image::ImageError),
    #[error("cannot render SVG: {0}")]
    Svg(#[from] resvg::usvg::Error),
    #[error("cannot encode thumbnail PNG: {0}")]
    Png(#[from] png::EncodingError),
    #[error("thumbnail generation previously failed for this file")]
    CachedFailure,
    #[error("the audio file has no embedded cover")]
    NoCover,
    #[error(transparent)]
    Audio(#[from] super::AudioError),
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

impl From<::image::ImageError> for Error {
    fn from(error: ::image::ImageError) -> Self {
        match error {
            ::image::ImageError::Limits(_) => Self::TooLarge,
            error => Self::Image(error),
        }
    }
}
