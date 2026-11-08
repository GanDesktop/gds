//! Audio tags, stream properties and embedded artwork, without decoding audio.
use std::{path::Path, time::Duration};

use lofty::{
    config::ParseOptions,
    file::{AudioFile, FileType, TaggedFile, TaggedFileExt},
    picture::PictureType,
    probe::Probe,
    tag::Accessor,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub format: &'static str,
    pub tags: Tags,
    pub duration: Option<Duration>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub bit_depth: Option<u16>,
    /// Audio bitrate in bits per second.
    pub bitrate: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read audio: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot parse audio metadata: {0}")]
    Parse(#[from] lofty::error::FileParseError),
    #[error("the audio source is not a regular file")]
    NotFile,
}

impl Metadata {
    /// Read tags and stream properties, skipping artwork allocations.
    ///
    /// # Errors
    /// Reports inaccessible, malformed or unsupported audio files.
    pub(super) fn read(path: &Path) -> Result<Self, Error> {
        let file = read(path, ParseOptions::new().read_cover_art(false))?;
        Ok(Self::from_file(&file))
    }

    pub(super) fn from_file(file: &TaggedFile) -> Self {
        let properties = file.properties();
        let mut tags = Tags::default();
        // Prefer the native tag, filling missing fields from secondary tags.
        for tag in file.primary_tag().into_iter().chain(file.tags()) {
            tags.title = tags.title.or_else(|| tag.title().map(|s| s.into_owned()));
            tags.artist = tags.artist.or_else(|| tag.artist().map(|s| s.into_owned()));
            tags.album = tags.album.or_else(|| tag.album().map(|s| s.into_owned()));
            tags.genre = tags.genre.or_else(|| tag.genre().map(|s| s.into_owned()));
            tags.year = tags
                .year
                .or_else(|| tag.date().map(|date| u32::from(date.year)));
            tags.track = tags.track.or_else(|| tag.track());
        }
        let format = match file.file_type() {
            FileType::Aac => "AAC",
            FileType::Aiff => "AIFF",
            FileType::Ape => "Monkey’s Audio",
            FileType::Flac => "FLAC",
            FileType::Mpeg => "MPEG Audio",
            FileType::Mp4 => "MPEG-4 Audio",
            FileType::Mpc => "Musepack",
            FileType::Opus => "Opus",
            FileType::Vorbis => "Ogg Vorbis",
            FileType::Speex => "Speex",
            FileType::Wav => "WAV",
            FileType::WavPack => "WavPack",
            _ => "Audio",
        };
        Self {
            format,
            tags,
            duration: (!properties.duration().is_zero()).then_some(properties.duration()),
            sample_rate: properties.sample_rate(),
            channels: properties.channels().map(u16::from),
            bit_depth: properties.bit_depth().map(u16::from),
            bitrate: properties
                .audio_bitrate()
                .map(|rate| u64::from(rate) * 1000),
        }
    }
}

/// Extract the front cover, falling back to the first embedded picture.
///
/// # Errors
/// Reports inaccessible, malformed or unsupported audio files.
pub fn cover(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    let file = read(path, ParseOptions::new().read_properties(false))?;
    Ok(picture(&file).map(|picture| picture.data().to_vec()))
}

pub(super) fn picture(file: &TaggedFile) -> Option<&lofty::picture::Picture> {
    let mut pictures = file.tags().iter().flat_map(|tag| tag.pictures());
    pictures
        .clone()
        .find(|picture| picture.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.next())
}

/// # Errors
/// Reports non-regular files, I/O failures and metadata parsing failures.
pub(super) fn read(path: &Path, options: ParseOptions) -> Result<TaggedFile, Error> {
    if !path.metadata()?.is_file() {
        return Err(Error::NotFile);
    }
    Ok(Probe::open(path)?
        .guess_file_type()?
        .options(options)
        .read()?)
}
