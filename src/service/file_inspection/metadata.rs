//! Typed content metadata and bounded header probes.
//!
//! Signature probes read at most [`PREFIX_BYTES`] from the start of a regular file;
//! ISO-BMFF containers walk box headers with `Seek` so trailing `moov` metadata is
//! found without reading `mdat` payloads or sample tables.
//! Thumbnail decoding runs independently in the shared thumbnail service.
//! Parsers never walk past their bounds: a truncated header simply yields fewer
//! fields instead of a failure.
//!
//! Formats whose metadata lives only in unindexed trailers (Ogg granule of the last
//! page, PDF trailers, ZIP central directories) stay `None`.

use std::{
    fs::{self, File},
    io::{self, BufReader, Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};

use image::{ExtendedColorType, ImageDecoder, ImageReader, Limits};
use mp4_atom::{
    Atom as _, Audio as IsoAudioEntry, Decode as _, DecodeMaybe as _, FourCC, Ftyp, Hdlr,
    Header as IsoHeader, Mdhd, Mdia, Minf, Moov, Mvhd, ReadFrom, Stbl, Stsd, Tkhd, Trak,
    Visual as IsoVisualEntry,
};

/// Bytes sampled from a file for format and header metadata.
const PREFIX_BYTES: usize = 256 * 1024;

/// Maximum leaf atom payload buffered when probing ISO-BMFF headers.
const MAX_ISO_LEAF_BYTES: u64 = 64 * 1024;

/// Maximum sibling boxes inspected in a single ISO-BMFF container.
const MAX_ISO_SIBLINGS: usize = 256;

/// Maximum tracks recorded from a movie header.
const MAX_ISO_TRACKS: usize = 64;

/// Direct children inspected for a folder; counting stops at this bound.
const DIRECTORY_CHILD_LIMIT: usize = 256;

/// Content information from a file, directory or symbolic link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Metadata {
    File(FileFacts),
    Directory(DirectoryFacts),
    Symlink(SymlinkFacts),
}

/// Type-specific content information from a regular file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileFacts {
    Image(ImageFacts),
    Audio(AudioFacts),
    Video(VideoFacts),
    Text(TextFacts),
    /// Bytes were read but no supported structure was recognised.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageFacts {
    pub format: ImageKind,
    pub size: Option<PixelSize>,
    pub bit_depth: Option<u8>,
    /// Only reported for formats whose header states transparency structurally.
    pub alpha: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelSize {
    pub width: u32,
    pub height: u32,
}

impl PixelSize {
    fn new(width: u32, height: u32) -> Option<Self> {
        (width > 0 && height > 0).then_some(Self { width, height })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
    Tiff,
    Ico,
    Pnm,
    Tga,
    Dds,
    OpenExr,
    RadianceHdr,
    Farbfeld,
    Avif,
    Qoi,
}

impl ImageKind {
    fn from_format(format: image::ImageFormat) -> Option<Self> {
        Some(match format {
            image::ImageFormat::Png => Self::Png,
            image::ImageFormat::Jpeg => Self::Jpeg,
            image::ImageFormat::Gif => Self::Gif,
            image::ImageFormat::WebP => Self::WebP,
            image::ImageFormat::Bmp => Self::Bmp,
            image::ImageFormat::Tiff => Self::Tiff,
            image::ImageFormat::Ico => Self::Ico,
            image::ImageFormat::Pnm => Self::Pnm,
            image::ImageFormat::Tga => Self::Tga,
            image::ImageFormat::Dds => Self::Dds,
            image::ImageFormat::OpenExr => Self::OpenExr,
            image::ImageFormat::Hdr => Self::RadianceHdr,
            image::ImageFormat::Farbfeld => Self::Farbfeld,
            image::ImageFormat::Avif => Self::Avif,
            image::ImageFormat::Qoi => Self::Qoi,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::WebP => "WebP",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
            Self::Ico => "ICO",
            Self::Pnm => "PNM",
            Self::Tga => "TGA",
            Self::Dds => "DDS",
            Self::OpenExr => "OpenEXR",
            Self::RadianceHdr => "Radiance HDR",
            Self::Farbfeld => "Farbfeld",
            Self::Avif => "AVIF",
            Self::Qoi => "QOI",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioFacts {
    pub format: AudioKind,
    pub tags: super::audio::Tags,
    pub duration: Option<Duration>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub bit_depth: Option<u16>,
    pub bitrate: Option<u64>,
}

impl From<super::audio::Metadata> for AudioFacts {
    fn from(metadata: super::audio::Metadata) -> Self {
        Self {
            format: AudioKind::Tagged(metadata.format),
            tags: metadata.tags,
            duration: metadata.duration,
            sample_rate: metadata.sample_rate,
            channels: metadata.channels,
            bit_depth: metadata.bit_depth,
            bitrate: metadata.bitrate,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioKind {
    Wav,
    Flac,
    OggVorbis,
    Opus,
    Mp3,
    M4a,
    Tagged(&'static str),
}

impl AudioKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::OggVorbis => "Ogg Vorbis",
            Self::Opus => "Opus",
            Self::Mp3 => "MP3",
            Self::M4a => "M4A",
            Self::Tagged(format) => format,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoFacts {
    pub format: VideoKind,
    pub duration: Option<Duration>,
    pub dimensions: Option<PixelSize>,
    pub codec: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoKind {
    Mp4,
    QuickTime,
    ThreeGpp,
    Avi,
    WebM,
    Matroska,
}

impl VideoKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MPEG-4",
            Self::QuickTime => "QuickTime",
            Self::ThreeGpp => "3GPP",
            Self::Avi => "AVI",
            Self::WebM => "WebM",
            Self::Matroska => "Matroska",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFacts {
    pub encoding: TextEncoding,
    pub line_endings: Option<LineEndings>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

impl TextEncoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Utf32Le => "UTF-32 LE",
            Self::Utf32Be => "UTF-32 BE",
        }
    }

    fn is_utf8(self) -> bool {
        matches!(self, Self::Utf8 | Self::Utf8Bom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEndings {
    Lf,
    Crlf,
    Cr,
    Mixed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectoryFacts {
    pub folders: u32,
    pub files: u32,
    pub links: u32,
    /// The directory holds more children than [`DIRECTORY_CHILD_LIMIT`].
    pub capped: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymlinkFacts {
    pub target: PathBuf,
}

/// Inspect a regular file from disk, bounding signature reads to [`PREFIX_BYTES`]
/// and walking ISO-BMFF box headers via `Seek`.
///
/// Directories, FIFOs and device nodes are refused before opening so a preview
/// never blocks on a special file.
///
/// # Errors
/// Returns the I/O error when the path is not a regular file or cannot be read.
pub(super) fn probe_path(path: &Path, size: Option<u64>, hint: ProbeHint) -> io::Result<Metadata> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let file_len = metadata.len();
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut prefix = Vec::with_capacity(file_len.min(PREFIX_BYTES as u64) as usize);
    reader
        .by_ref()
        .take(PREFIX_BYTES as u64)
        .read_to_end(&mut prefix)?;
    Ok(probe_reader(
        size.or(Some(file_len)),
        hint,
        &prefix,
        &mut reader,
        file_len,
    ))
}

/// What the directory listing already believes about a file's content.
///
/// These hints gate the weak signatures: text is plain bytes and an MP3 frame
/// sync can occur inside any binary, so neither is claimed without them.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ProbeHint {
    pub text: bool,
    pub audio: bool,
}

#[cfg(test)]
fn probe_file(size: Option<u64>, hint: ProbeHint, bytes: &[u8]) -> Metadata {
    let prefix = &bytes[..bytes.len().min(PREFIX_BYTES)];
    let mut cursor = Cursor::new(bytes);
    probe_reader(size, hint, prefix, &mut cursor, bytes.len() as u64)
}

/// Inspect a regular file from its initial prefix and seekable stream.
///
/// `hint` comes from the directory listing's content category and keeps
/// arbitrary binaries from being presented as text or MP3.
fn probe_reader<R: Read + Seek>(
    size: Option<u64>,
    hint: ProbeHint,
    prefix: &[u8],
    reader: &mut R,
    stream_len: u64,
) -> Metadata {
    if prefix.is_empty() {
        return Metadata::File(FileFacts::Unknown);
    }
    if let Ok(format) = image::guess_format(prefix)
        && let Some(image) = image_facts(format, prefix)
    {
        return Metadata::File(FileFacts::Image(image));
    }
    if let Some(audio) = prefix_audio_facts(prefix, size) {
        return Metadata::File(FileFacts::Audio(audio));
    }
    if let Some(video) = container_video_facts(prefix) {
        return Metadata::File(FileFacts::Video(video));
    }
    if is_iso_start(prefix)
        && let Some(iso) = iso_facts(reader, stream_len)
    {
        if iso_is_audio(iso.brand, &iso.info) {
            return Metadata::File(FileFacts::Audio(audio_from_iso(&iso.info, size)));
        }
        return Metadata::File(FileFacts::Video(video_from_iso(&iso)));
    }
    if hint.audio
        && let Some(mp3) = mp3_facts(prefix, size)
    {
        return Metadata::File(FileFacts::Audio(mp3));
    }
    if hint.text
        && let Some(text) = text_facts(prefix, prefix.len() == PREFIX_BYTES)
    {
        return Metadata::File(FileFacts::Text(text));
    }
    Metadata::File(FileFacts::Unknown)
}

/// Count one level of direct children, never recursing.
/// # Errors
/// Reports inaccessible directories or entries whose type cannot be read.
pub(super) fn probe_directory(path: &Path) -> io::Result<Metadata> {
    let mut facts = DirectoryFacts::default();
    let mut entries = fs::read_dir(path)?;
    for entry in entries.by_ref().take(DIRECTORY_CHILD_LIMIT) {
        let file_type = entry?.file_type()?;
        if file_type.is_dir() {
            facts.folders += 1;
        } else if file_type.is_symlink() {
            facts.links += 1;
        } else {
            facts.files += 1;
        }
    }
    facts.capped = entries.next().transpose()?.is_some();
    Ok(Metadata::Directory(facts))
}

fn image_facts(format: image::ImageFormat, bytes: &[u8]) -> Option<ImageFacts> {
    let kind = ImageKind::from_format(format)?;
    let mut facts = ImageFacts {
        format: kind,
        size: None,
        bit_depth: None,
        alpha: None,
    };
    if kind == ImageKind::Png {
        probe_png(&mut facts, bytes);
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    if let Ok(decoder) = reader.into_decoder() {
        let (width, height) = decoder.dimensions();
        facts.size = facts.size.or_else(|| PixelSize::new(width, height));
        if kind != ImageKind::Png {
            let color = decoder.original_color_type();
            facts.bit_depth = bits_per_channel(color);
            if kind == ImageKind::WebP {
                facts.alpha = Some(has_alpha(color));
            }
        }
    }
    Some(facts)
}

/// PNG states bit depth, colour type and palette transparency in IHDR/tRNS.
fn probe_png(facts: &mut ImageFacts, bytes: &[u8]) {
    let decoder = png_header_decoder(bytes);
    match decoder.read_info() {
        Ok(reader) => {
            let info = reader.info();
            apply_png_info(facts, info, Some(info.trns.is_some()));
        }
        Err(_) => {
            // Prefixes truncated before IDAT fail read_info(); fall back to IHDR so
            // dimensions, bit depth and non-indexed alpha still appear when valid.
            let mut header = png_header_decoder(bytes);
            if let Ok(info) = header.read_header_info() {
                apply_png_info(facts, info, None);
            }
        }
    }
}

fn png_header_decoder(bytes: &[u8]) -> png::Decoder<Cursor<&[u8]>> {
    let mut decoder =
        png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: 1024 * 1024 });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder
}

fn apply_png_info(facts: &mut ImageFacts, info: &png::Info<'_>, trns: Option<bool>) {
    facts.size = PixelSize::new(info.width, info.height);
    facts.bit_depth = Some(match info.bit_depth {
        png::BitDepth::One => 1,
        png::BitDepth::Two => 2,
        png::BitDepth::Four => 4,
        png::BitDepth::Eight => 8,
        png::BitDepth::Sixteen => 16,
    });
    facts.alpha = match info.color_type {
        png::ColorType::GrayscaleAlpha | png::ColorType::Rgba => Some(true),
        png::ColorType::Grayscale | png::ColorType::Rgb => Some(false),
        png::ColorType::Indexed => trns,
    };
}

fn bits_per_channel(color: ExtendedColorType) -> Option<u8> {
    let channels = color.channel_count() as u16;
    let bits = color.bits_per_pixel();
    (channels > 0 && bits % channels == 0).then_some((bits / channels) as u8)
}

fn has_alpha(color: ExtendedColorType) -> bool {
    matches!(
        color,
        ExtendedColorType::A8
            | ExtendedColorType::La1
            | ExtendedColorType::La2
            | ExtendedColorType::La4
            | ExtendedColorType::La8
            | ExtendedColorType::La16
            | ExtendedColorType::Rgba1
            | ExtendedColorType::Rgba2
            | ExtendedColorType::Rgba4
            | ExtendedColorType::Rgba8
            | ExtendedColorType::Rgba16
            | ExtendedColorType::Rgba32F
            | ExtendedColorType::Bgra8
    )
}

fn prefix_audio_facts(bytes: &[u8], size: Option<u64>) -> Option<AudioFacts> {
    if let Some(facts) = wav_facts(bytes) {
        return Some(facts);
    }
    if let Some(facts) = flac_facts(bytes, size) {
        return Some(facts);
    }
    ogg_facts(bytes)
}

fn wav_facts(bytes: &[u8]) -> Option<AudioFacts> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut facts = AudioFacts {
        format: AudioKind::Wav,
        tags: Default::default(),
        duration: None,
        sample_rate: None,
        channels: None,
        bit_depth: None,
        bitrate: None,
    };
    let mut byte_rate = None;
    let mut data_size = None;
    let mut offset = 12;
    while let Some(header) = bytes.get(offset..offset + 8) {
        let id = &header[..4];
        let length = u32_le(header, 4)? as usize;
        let body = offset + 8;
        if id == b"fmt " {
            if let Some(chunk) = bytes.get(body..body.saturating_add(length).min(bytes.len())) {
                facts.channels = u16_le(chunk, 2);
                facts.sample_rate = u32_le(chunk, 4);
                byte_rate = u32_le(chunk, 8);
                facts.bit_depth = u16_le(chunk, 14);
                facts.bitrate = byte_rate.map(|rate| u64::from(rate) * 8);
            }
        } else if id == b"data" {
            data_size = Some(length as u64);
        }
        offset = body.checked_add(length + (length & 1))?;
        if data_size.is_some() && facts.sample_rate.is_some() {
            break;
        }
    }
    facts.duration = duration_of(byte_rate, data_size);
    Some(facts)
}

fn flac_facts(bytes: &[u8], size: Option<u64>) -> Option<AudioFacts> {
    if bytes.get(..4)? != b"fLaC" {
        return None;
    }
    let block = bytes.get(4..8)?;
    let block_type = block[0] & 0x7f;
    let length = ((block[1] as usize) << 16) | ((block[2] as usize) << 8) | block[3] as usize;
    if block_type != 0 || length < 34 {
        return Some(AudioFacts {
            format: AudioKind::Flac,
            tags: Default::default(),
            duration: None,
            sample_rate: None,
            channels: None,
            bit_depth: None,
            bitrate: None,
        });
    }
    let streaminfo = bytes.get(8..8 + 34)?;
    let packed = u64_be(streaminfo, 10)?;
    let sample_rate = ((packed >> 44) & 0x000f_ffff) as u32;
    let channels = (((packed >> 41) & 0x7) + 1) as u16;
    let bit_depth = (((packed >> 36) & 0x1f) + 1) as u16;
    let total_samples = packed & 0x0000_000f_ffff_ffff;
    let duration = duration_of(Some(sample_rate), Some(total_samples));
    Some(AudioFacts {
        format: AudioKind::Flac,
        tags: Default::default(),
        duration,
        sample_rate: (sample_rate > 0).then_some(sample_rate),
        channels: Some(channels),
        bit_depth: Some(bit_depth),
        bitrate: average_bitrate(size, duration),
    })
}

fn ogg_facts(bytes: &[u8]) -> Option<AudioFacts> {
    let packet = ogg_first_packet(bytes)?;
    if packet.starts_with(b"OpusHead") {
        return Some(AudioFacts {
            format: AudioKind::Opus,
            tags: Default::default(),
            duration: None,
            sample_rate: Some(48_000),
            channels: packet.get(9).map(|&channels| u16::from(channels)),
            bit_depth: None,
            bitrate: None,
        });
    }
    let vorbis = packet.get(..7)? == b"\x01vorbis";
    if !vorbis {
        return None;
    }
    let sample_rate = u32_le(packet, 12);
    let nominal = i32_le(packet, 20).filter(|rate| *rate > 0);
    Some(AudioFacts {
        format: AudioKind::OggVorbis,
        tags: Default::default(),
        duration: None,
        sample_rate: sample_rate.filter(|rate| *rate > 0),
        channels: packet.get(11).map(|&channels| u16::from(channels)),
        bit_depth: None,
        bitrate: nominal.map(|rate| rate as u64),
    })
}

fn ogg_first_packet(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.get(..4)? != b"OggS" {
        return None;
    }
    let segments = usize::from(*bytes.get(26)?);
    let table_end = 27 + segments;
    let table = bytes.get(27..table_end)?;
    let body_len: usize = table.iter().map(|&segment| usize::from(segment)).sum();
    let end = table_end.checked_add(body_len)?;
    Some(bytes.get(table_end..end.min(bytes.len()))?)
}

fn mp3_facts(bytes: &[u8], size: Option<u64>) -> Option<AudioFacts> {
    let start = if bytes.starts_with(b"ID3") {
        10 + syncsafe(bytes.get(6..10)?)
    } else {
        0
    };
    let search = bytes.get(start..start.saturating_add(64 * 1024).min(bytes.len()))?;
    let offset = search
        .windows(2)
        .position(|pair| pair[0] == 0xff && pair[1] & 0xe0 == 0xe0)?;
    let frame = parse_mp3_frame(bytes.get(start + offset..)?)?;
    let duration = mp3_duration(bytes, start + offset, &frame);
    let bitrate = average_bitrate(size, duration).or(frame.bitrate);
    Some(AudioFacts {
        format: AudioKind::Mp3,
        tags: Default::default(),
        duration,
        sample_rate: Some(frame.sample_rate),
        channels: Some(frame.channels),
        bit_depth: None,
        bitrate,
    })
}

struct Mp3Frame {
    bitrate: Option<u64>,
    sample_rate: u32,
    channels: u16,
    samples_per_frame: u32,
    side_info: usize,
}

fn parse_mp3_frame(bytes: &[u8]) -> Option<Mp3Frame> {
    let header = bytes.get(..4)?;
    let version = (header[1] >> 3) & 0x3;
    let layer = (header[1] >> 1) & 0x3;
    if version == 1 || layer == 0 {
        return None;
    }
    let bitrate_index = usize::from(header[2] >> 4);
    let sample_rate_index = usize::from((header[2] >> 2) & 0x3);
    if bitrate_index == 0 || bitrate_index == 15 || sample_rate_index == 3 {
        return None;
    }
    let bitrate = mp3_bitrate(version, layer, bitrate_index)?;
    let sample_rate = mp3_sample_rate(version, sample_rate_index)?;
    let channels = if header[3] >> 6 == 3 { 1 } else { 2 };
    let side_info = if layer == 1 {
        let mono = channels == 1;
        match (version, mono) {
            (3, true) => 17,
            (3, false) => 32,
            (_, true) => 9,
            (_, false) => 17,
        }
    } else {
        0
    };
    Some(Mp3Frame {
        bitrate: Some(u64::from(bitrate.kbps) * 1000),
        sample_rate,
        channels,
        samples_per_frame: bitrate.samples_per_frame,
        side_info,
    })
}

fn mp3_bitrate(version: u8, layer: u8, index: usize) -> Option<Mp3Bitrate> {
    const MPEG1_LAYER1: [u32; 15] = [
        0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
    ];
    const MPEG1_LAYER2: [u32; 15] = [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
    ];
    const MPEG1_LAYER3: [u32; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const MPEG2_LAYER1: [u32; 15] = [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
    ];
    const MPEG2_LAYER23: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let samples_per_frame = match layer {
        3 => 384,
        2 => 1152,
        _ if version == 3 => 1152,
        _ => 576,
    };
    let bitrate = match (version, layer) {
        (3, 3) => MPEG1_LAYER1[index],
        (3, 2) => MPEG1_LAYER2[index],
        (3, 1) => MPEG1_LAYER3[index],
        (_, 3) => MPEG2_LAYER1[index],
        _ => MPEG2_LAYER23[index],
    };
    Some(Mp3Bitrate {
        kbps: bitrate,
        samples_per_frame,
    })
}

fn mp3_sample_rate(version: u8, index: usize) -> Option<u32> {
    const MPEG1: [u32; 3] = [44_100, 48_000, 32_000];
    const MPEG2: [u32; 3] = [22_050, 24_000, 16_000];
    const MPEG25: [u32; 3] = [11_025, 12_000, 8_000];
    let table = match version {
        3 => MPEG1,
        2 => MPEG2,
        0 => MPEG25,
        _ => return None,
    };
    table.get(index).copied()
}

/// A Xing/Info header carries a frame count, which gives an exact duration.
fn mp3_duration(bytes: &[u8], frame_start: usize, frame: &Mp3Frame) -> Option<Duration> {
    let tag = frame_start + 4 + frame.side_info;
    let kind = bytes.get(tag..tag + 4)?;
    if kind != b"Xing" && kind != b"Info" {
        return None;
    }
    let flags = u32_be(bytes, tag + 4)?;
    if flags & 0x1 == 0 {
        return None;
    }
    let frames = u32_be(bytes, tag + 8)?;
    let total_samples = u64::from(frames) * u64::from(frame.samples_per_frame);
    duration_of(Some(frame.sample_rate), Some(total_samples))
}

fn average_bitrate(size: Option<u64>, duration: Option<Duration>) -> Option<u64> {
    let size = size?;
    let duration = duration?.as_secs_f64();
    (duration > 0.0).then(|| (size as f64 * 8.0 / duration) as u64)
}

struct Mp3Bitrate {
    kbps: u32,
    samples_per_frame: u32,
}

struct IsoFile {
    brand: [u8; 4],
    info: IsoInfo,
}

struct IsoTrack {
    handler: Option<[u8; 4]>,
    timescale: Option<u32>,
    duration: Option<u64>,
    codec: Option<[u8; 4]>,
    dimensions: Option<PixelSize>,
    channels: Option<u16>,
    bit_depth: Option<u16>,
    sample_rate: Option<u32>,
}

#[derive(Default)]
struct IsoInfo {
    movie_timescale: Option<u32>,
    movie_duration: Option<u64>,
    tracks: Vec<IsoTrack>,
}

struct BoxSpan {
    kind: FourCC,
    body_start: u64,
    box_end: u64,
}

impl BoxSpan {
    fn body_len(&self) -> u64 {
        self.box_end - self.body_start
    }
}

fn is_iso_start(prefix: &[u8]) -> bool {
    let mut slice = prefix;
    let Ok(Some(header)) = IsoHeader::decode_maybe(&mut slice) else {
        return false;
    };
    matches!(
        &<[u8; 4]>::from(header.kind),
        b"ftyp" | b"styp" | b"pdin" | b"free" | b"skip" | b"wide" | b"moov" | b"mdat"
    )
}

fn seek_to<R: Read + Seek>(reader: &mut R, target: u64) -> Option<()> {
    let current = reader.stream_position().ok()?;
    if current == target {
        return Some(());
    }
    if let Ok(delta) = i64::try_from(i128::from(target) - i128::from(current)) {
        reader.seek(SeekFrom::Current(delta)).ok()?;
    } else {
        reader.seek(SeekFrom::Start(target)).ok()?;
    }
    Some(())
}

fn next_box<R: Read + Seek>(reader: &mut R, cursor: &mut u64, container_end: u64) -> Option<BoxSpan> {
    if *cursor >= container_end || container_end - *cursor < 8 {
        return None;
    }
    seek_to(reader, *cursor)?;
    let header = <Option<IsoHeader> as ReadFrom>::read_from(reader).ok()??;
    let body_start = reader.stream_position().ok()?;
    let box_end = match header.size {
        Some(payload) => body_start.checked_add(u64::try_from(payload).ok()?)?,
        None => container_end,
    };
    if box_end < body_start || box_end > container_end {
        return None;
    }
    *cursor = box_end;
    Some(BoxSpan {
        kind: header.kind,
        body_start,
        box_end,
    })
}

fn read_leaf_body<R: Read + Seek>(reader: &mut R, span: &BoxSpan) -> Option<Vec<u8>> {
    let len = span.body_len();
    if len > MAX_ISO_LEAF_BYTES {
        return None;
    }
    let len_usize = usize::try_from(len).ok()?;
    seek_to(reader, span.body_start)?;
    let mut body = vec![0u8; len_usize];
    reader.read_exact(&mut body).ok()?;
    Some(body)
}

fn iso_facts<R: Read + Seek>(reader: &mut R, stream_len: u64) -> Option<IsoFile> {
    let mut major = None;
    let mut has_moov = false;
    let mut info = IsoInfo::default();
    let mut cursor = 0;
    for _ in 0..MAX_ISO_SIBLINGS {
        let Some(span) = next_box(reader, &mut cursor, stream_len) else {
            break;
        };
        if span.kind == Ftyp::KIND && major.is_none() {
            if let Some(body) = read_leaf_body(reader, &span) {
                major = decode_ftyp(&body);
            }
        } else if span.kind == Moov::KIND {
            read_moov(reader, span.body_start, span.box_end, &mut info);
            has_moov = true;
        }
        if major.is_some() && has_moov {
            break;
        }
    }
    Some(IsoFile {
        brand: major?,
        info,
    })
}

fn decode_ftyp(body: &[u8]) -> Option<[u8; 4]> {
    let mut slice = body;
    if let Ok(ftyp) = Ftyp::decode_body(&mut slice) {
        return Some(ftyp.major_brand.into());
    }
    body.get(..4)?.try_into().ok()
}

fn read_moov<R: Read + Seek>(reader: &mut R, start: u64, end: u64, info: &mut IsoInfo) {
    let mut cursor = start;
    for _ in 0..MAX_ISO_SIBLINGS {
        let Some(span) = next_box(reader, &mut cursor, end) else {
            break;
        };
        if span.kind == Mvhd::KIND {
            if let Some(body) = read_leaf_body(reader, &span)
                && let Some(&version) = body.first()
            {
                let mut slice = body.as_slice();
                if let Ok(mvhd) = Mvhd::decode_body(&mut slice) {
                    info.movie_timescale = Some(mvhd.timescale);
                    info.movie_duration = clean_iso_duration(version, mvhd.duration);
                }
            }
        } else if span.kind == Trak::KIND && info.tracks.len() < MAX_ISO_TRACKS {
            info.tracks
                .push(read_trak(reader, span.body_start, span.box_end));
        }
    }
}

fn read_trak<R: Read + Seek>(reader: &mut R, start: u64, end: u64) -> IsoTrack {
    let mut track = IsoTrack {
        handler: None,
        timescale: None,
        duration: None,
        codec: None,
        dimensions: None,
        channels: None,
        bit_depth: None,
        sample_rate: None,
    };
    let mut cursor = start;
    for _ in 0..MAX_ISO_SIBLINGS {
        let Some(span) = next_box(reader, &mut cursor, end) else {
            break;
        };
        if span.kind == Tkhd::KIND {
            if let Some(body) = read_leaf_body(reader, &span) {
                let mut slice = body.as_slice();
                if let Ok(tkhd) = Tkhd::decode_body(&mut slice)
                    && !tkhd.size_is_aspect_ratio
                {
                    track.dimensions = PixelSize::new(
                        u32::from(tkhd.width.integer()),
                        u32::from(tkhd.height.integer()),
                    );
                }
            }
        } else if span.kind == Mdia::KIND {
            read_mdia(reader, span.body_start, span.box_end, &mut track);
        }
    }
    track
}

fn read_mdia<R: Read + Seek>(reader: &mut R, start: u64, end: u64, track: &mut IsoTrack) {
    let mut cursor = start;
    for _ in 0..MAX_ISO_SIBLINGS {
        let Some(span) = next_box(reader, &mut cursor, end) else {
            break;
        };
        if span.kind == Mdhd::KIND {
            if let Some(body) = read_leaf_body(reader, &span)
                && let Some(&version) = body.first()
            {
                let mut slice = body.as_slice();
                if let Ok(mdhd) = Mdhd::decode_body(&mut slice) {
                    track.timescale = Some(mdhd.timescale);
                    track.duration = clean_iso_duration(version, mdhd.duration);
                }
            }
        } else if span.kind == Hdlr::KIND {
            if let Some(body) = read_leaf_body(reader, &span) {
                let mut slice = body.as_slice();
                if let Ok(hdlr) = Hdlr::decode_body(&mut slice) {
                    track.handler = Some(hdlr.handler.into());
                }
            }
        } else if span.kind == Minf::KIND {
            read_minf(reader, span.body_start, span.box_end, track);
        }
    }
}

fn read_minf<R: Read + Seek>(reader: &mut R, start: u64, end: u64, track: &mut IsoTrack) {
    let mut cursor = start;
    for _ in 0..MAX_ISO_SIBLINGS {
        let Some(span) = next_box(reader, &mut cursor, end) else {
            break;
        };
        if span.kind == Stbl::KIND {
            read_stbl(reader, span.body_start, span.box_end, track);
        }
    }
}

fn read_stbl<R: Read + Seek>(reader: &mut R, start: u64, end: u64, track: &mut IsoTrack) {
    let mut cursor = start;
    for _ in 0..MAX_ISO_SIBLINGS {
        let Some(span) = next_box(reader, &mut cursor, end) else {
            break;
        };
        if span.kind == Stsd::KIND
            && let Some(body) = read_leaf_body(reader, &span)
        {
            read_stsd(&body, track);
        }
    }
}

/// Only the first sample entry is inspected; a file with several entries still
/// reports its main track codec.
fn read_stsd(payload: &[u8], track: &mut IsoTrack) {
    if u32_be(payload, 4).unwrap_or(0) == 0 {
        return;
    }
    let Some(mut rest) = payload.get(8..) else {
        return;
    };
    let Ok(Some(header)) = IsoHeader::decode_maybe(&mut rest) else {
        return;
    };
    let entry_len = header.size.unwrap_or(rest.len()).min(rest.len());
    let mut entry = &rest[..entry_len];
    let kind: [u8; 4] = header.kind.into();
    track.codec = Some(kind);
    match &kind {
        b"avc1" | b"avc3" | b"hvc1" | b"hev1" | b"av01" | b"vp09" | b"mp4v" | b"apch"
        | b"apcn" | b"apcs" | b"apco" | b"ap4h" => {
            if let Ok(visual) = IsoVisualEntry::decode(&mut entry) {
                track.dimensions =
                    PixelSize::new(u32::from(visual.width), u32::from(visual.height))
                        .or(track.dimensions);
            }
        }
        b"mp4a" | b"alac" | b"Opus" | b"fLaC" => {
            if let Ok(audio) = IsoAudioEntry::decode(&mut entry) {
                track.channels = Some(audio.channel_count);
                track.bit_depth = Some(audio.sample_size);
                track.sample_rate = Some(u32::from(audio.sample_rate.integer()));
            }
        }
        _ => {}
    }
}

/// ISO/IEC 14496-12 reserves all-ones (`u32::MAX` in v0, `u64::MAX` in v1) for
/// indeterminate movie and media durations.
fn clean_iso_duration(version: u8, duration: u64) -> Option<u64> {
    let indeterminate = (version == 0 && duration == u64::from(u32::MAX)) || duration == u64::MAX;
    (!indeterminate && duration > 0).then_some(duration)
}

fn iso_is_audio(brand: [u8; 4], info: &IsoInfo) -> bool {
    let has_video = info
        .tracks
        .iter()
        .any(|track| track.handler == Some(*b"vide") || track.dimensions.is_some());
    if has_video {
        return false;
    }
    let has_audio = info
        .tracks
        .iter()
        .any(|track| track.handler == Some(*b"soun") || track.channels.is_some());
    has_audio || brand == *b"M4A " || brand == *b"M4B "
}

fn iso_kind(brand: [u8; 4]) -> VideoKind {
    match &brand {
        b"qt  " => VideoKind::QuickTime,
        b"3gp4" | b"3gp5" | b"3gp6" | b"3g2a" => VideoKind::ThreeGpp,
        _ => VideoKind::Mp4,
    }
}

fn audio_from_iso(info: &IsoInfo, size: Option<u64>) -> AudioFacts {
    let track = info
        .tracks
        .iter()
        .find(|track| track.handler == Some(*b"soun"))
        .or_else(|| info.tracks.first());
    let duration = track
        .and_then(|track| duration_of(track.timescale, track.duration))
        .or_else(|| duration_of(info.movie_timescale, info.movie_duration));
    AudioFacts {
        format: AudioKind::M4a,
        tags: Default::default(),
        duration,
        sample_rate: track
            .and_then(|track| track.sample_rate)
            .filter(|rate| *rate > 0),
        channels: track.and_then(|track| track.channels),
        bit_depth: track.and_then(|track| track.bit_depth),
        bitrate: average_bitrate(size, duration),
    }
}

fn container_video_facts(bytes: &[u8]) -> Option<VideoFacts> {
    if bytes.get(..4)? == b"RIFF" && bytes.get(8..12)? == b"AVI " {
        return Some(VideoFacts {
            format: VideoKind::Avi,
            duration: None,
            dimensions: None,
            codec: None,
        });
    }
    if bytes.get(..4)? == b"\x1aE\xdf\xa3" {
        let webm = bytes
            .get(..4096.min(bytes.len()))
            .is_some_and(|head| head.windows(4).any(|window| window == b"webm"));
        return Some(VideoFacts {
            format: if webm {
                VideoKind::WebM
            } else {
                VideoKind::Matroska
            },
            duration: None,
            dimensions: None,
            codec: None,
        });
    }
    None
}

fn video_from_iso(iso: &IsoFile) -> VideoFacts {
    let info = &iso.info;
    let track = info
        .tracks
        .iter()
        .find(|track| track.handler == Some(*b"vide") || track.dimensions.is_some())
        .or_else(|| info.tracks.first());
    let duration = track
        .and_then(|track| duration_of(track.timescale, track.duration))
        .or_else(|| duration_of(info.movie_timescale, info.movie_duration));
    let dimensions = track.and_then(|track| track.dimensions);
    let codec = track.and_then(|track| track.codec).map(codec_label);
    VideoFacts {
        format: iso_kind(iso.brand),
        duration,
        dimensions,
        codec,
    }
}

fn duration_of(timescale: Option<u32>, units: Option<u64>) -> Option<Duration> {
    let scale = u64::from(timescale.filter(|value| *value > 0)?);
    let units = units.filter(|value| *value > 0)?;
    let secs = units / scale;
    let nanos = ((u128::from(units % scale) * 1_000_000_000) / u128::from(scale)) as u32;
    Some(Duration::new(secs, nanos))
}

fn codec_label(fourcc: [u8; 4]) -> String {
    let label = match &fourcc {
        b"avc1" | b"avc3" => "H.264",
        b"hvc1" | b"hev1" => "H.265",
        b"av01" => "AV1",
        b"vp09" => "VP9",
        b"mp4v" => "MPEG-4 Visual",
        b"apch" | b"apcn" | b"apcs" | b"apco" | b"ap4h" => "ProRes",
        b"mp4a" => "AAC",
        b"alac" => "ALAC",
        b"Opus" => "Opus",
        b"fLaC" => "FLAC",
        _ => return String::from_utf8_lossy(&fourcc).into_owned(),
    };
    label.to_owned()
}

fn text_facts(bytes: &[u8], truncated: bool) -> Option<TextFacts> {
    if bytes.is_empty() {
        return None;
    }
    if let Some(mark) = byte_order_mark(bytes) {
        let encoding = mark.encoding;
        let skip = mark.length;
        if encoding.is_utf8() {
            let body = bytes.get(skip..)?;
            let line_endings = if valid_utf8_sample(body) {
                line_endings(body, truncated)
            } else {
                None
            };
            return Some(TextFacts {
                encoding,
                line_endings,
            });
        }
        return Some(TextFacts {
            encoding,
            line_endings: None,
        });
    }
    if !valid_utf8_sample(bytes) {
        return None;
    }
    Some(TextFacts {
        encoding: TextEncoding::Utf8,
        line_endings: line_endings(bytes, truncated),
    })
}

struct ByteOrderMark {
    encoding: TextEncoding,
    length: usize,
}

fn byte_order_mark(bytes: &[u8]) -> Option<ByteOrderMark> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Some(ByteOrderMark {
            encoding: TextEncoding::Utf8Bom,
            length: 3,
        });
    }
    if bytes.starts_with(&[0xff, 0xfe, 0x00, 0x00]) {
        return Some(ByteOrderMark {
            encoding: TextEncoding::Utf32Le,
            length: 4,
        });
    }
    if bytes.starts_with(&[0x00, 0x00, 0xfe, 0xff]) {
        return Some(ByteOrderMark {
            encoding: TextEncoding::Utf32Be,
            length: 4,
        });
    }
    if bytes.starts_with(&[0xff, 0xfe]) {
        return Some(ByteOrderMark {
            encoding: TextEncoding::Utf16Le,
            length: 2,
        });
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        return Some(ByteOrderMark {
            encoding: TextEncoding::Utf16Be,
            length: 2,
        });
    }
    None
}

/// Valid UTF-8, allowing a multi-byte sequence that the prefix cut in half.
fn valid_utf8_sample(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(error) => error.valid_up_to() + 4 >= bytes.len(),
    }
}

fn line_endings(bytes: &[u8], truncated: bool) -> Option<LineEndings> {
    let mut lf = 0u32;
    let mut crlf = 0u32;
    let mut cr = 0u32;
    let mut index = 0;
    // A carriage return on the prefix boundary may be the first half of CRLF.
    let end = if truncated && bytes.last() == Some(&b'\r') {
        bytes.len().saturating_sub(1)
    } else {
        bytes.len()
    };
    while index < end {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                crlf += 1;
                index += 2;
            }
            b'\r' => {
                cr += 1;
                index += 1;
            }
            b'\n' => {
                lf += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    let total = lf + crlf + cr;
    if total == 0 {
        return None;
    }
    Some(if lf > 0 && crlf == 0 && cr == 0 {
        LineEndings::Lf
    } else if crlf > 0 && lf == 0 && cr == 0 {
        LineEndings::Crlf
    } else if cr > 0 && lf == 0 && crlf == 0 {
        LineEndings::Cr
    } else {
        LineEndings::Mixed
    })
}

fn syncsafe(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .take(4)
        .fold(0usize, |value, &byte| value << 7 | usize::from(byte & 0x7f))
}

fn u16_le(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn i32_le(bytes: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u32_be(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_be(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_png(
        width: u32,
        height: u32,
        color: png::ColorType,
        depth: png::BitDepth,
        palette: Option<&[u8]>,
        trns: Option<&[u8]>,
        pixels: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if let Some(palette) = palette {
            encoder.set_palette(palette);
        }
        if let Some(trns) = trns {
            encoder.set_trns(trns);
        }
        let mut writer = encoder.write_header().expect("PNG header");
        writer.write_image_data(pixels).expect("PNG image data");
        writer.finish().expect("PNG footer");
        bytes
    }

    fn probe_png_facts(bytes: &[u8]) -> ImageFacts {
        match probe_file(Some(bytes.len() as u64), ProbeHint::default(), bytes) {
            Metadata::File(FileFacts::Image(facts)) => facts,
            other => panic!("expected image metadata, got {other:?}"),
        }
    }

    #[test]
    fn png_header_probe_extracts_depth_dimensions_and_alpha() {
        let rgba = encode_png(
            3,
            2,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            None,
            None,
            &[0; 3 * 2 * 4],
        );
        assert_eq!(
            probe_png_facts(&rgba),
            ImageFacts {
                format: ImageKind::Png,
                size: PixelSize::new(3, 2),
                bit_depth: Some(8),
                alpha: Some(true),
            }
        );

        let rgb = encode_png(
            4,
            5,
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            None,
            None,
            &[0; 4 * 5 * 3],
        );
        assert_eq!(
            probe_png_facts(&rgb),
            ImageFacts {
                format: ImageKind::Png,
                size: PixelSize::new(4, 5),
                bit_depth: Some(8),
                alpha: Some(false),
            }
        );

        let indexed_opaque = encode_png(
            2,
            2,
            png::ColorType::Indexed,
            png::BitDepth::Eight,
            Some(&[10, 20, 30]),
            None,
            &[0; 4],
        );
        assert_eq!(
            probe_png_facts(&indexed_opaque),
            ImageFacts {
                format: ImageKind::Png,
                size: PixelSize::new(2, 2),
                bit_depth: Some(8),
                alpha: Some(false),
            }
        );

        let indexed_trns = encode_png(
            2,
            2,
            png::ColorType::Indexed,
            png::BitDepth::Eight,
            Some(&[10, 20, 30]),
            Some(&[128]),
            &[0; 4],
        );
        assert_eq!(
            probe_png_facts(&indexed_trns),
            ImageFacts {
                format: ImageKind::Png,
                size: PixelSize::new(2, 2),
                bit_depth: Some(8),
                alpha: Some(true),
            }
        );

        let rgb16 = encode_png(
            2,
            1,
            png::ColorType::Rgb,
            png::BitDepth::Sixteen,
            None,
            None,
            &[0; 2 * 3 * 2],
        );
        assert_eq!(
            probe_png_facts(&rgb16),
            ImageFacts {
                format: ImageKind::Png,
                size: PixelSize::new(2, 1),
                bit_depth: Some(16),
                alpha: Some(false),
            }
        );

        // 33 bytes = PNG signature (8) + IHDR chunk (25), truncated before PLTE/tRNS/IDAT.
        assert_eq!(
            probe_png_facts(&rgba[..33]),
            ImageFacts {
                format: ImageKind::Png,
                size: PixelSize::new(3, 2),
                bit_depth: Some(8),
                alpha: Some(true),
            }
        );
        assert_eq!(
            probe_png_facts(&rgb[..33]),
            ImageFacts {
                format: ImageKind::Png,
                size: PixelSize::new(4, 5),
                bit_depth: Some(8),
                alpha: Some(false),
            }
        );
        assert_eq!(
            probe_png_facts(&indexed_trns[..33]),
            ImageFacts {
                format: ImageKind::Png,
                size: PixelSize::new(2, 2),
                bit_depth: Some(8),
                alpha: None,
            }
        );
        // Truncated inside IDAT after header chunks preserves tRNS detection.
        let idat_offset = indexed_trns
            .windows(4)
            .position(|window| window == b"IDAT")
            .expect("IDAT chunk");
        assert_eq!(
            probe_png_facts(&indexed_trns[..idat_offset + 8]),
            ImageFacts {
                format: ImageKind::Png,
                size: PixelSize::new(2, 2),
                bit_depth: Some(8),
                alpha: Some(true),
            }
        );
    }

    fn probe_audio_facts(size: Option<u64>, hint: ProbeHint, bytes: &[u8]) -> AudioFacts {
        match probe_file(size, hint, bytes) {
            Metadata::File(FileFacts::Audio(facts)) => facts,
            other => panic!("expected audio metadata, got {other:?}"),
        }
    }

    fn probe_video_facts(bytes: &[u8]) -> VideoFacts {
        match probe_file(Some(bytes.len() as u64), ProbeHint::default(), bytes) {
            Metadata::File(FileFacts::Video(facts)) => facts,
            other => panic!("expected video metadata, got {other:?}"),
        }
    }

    fn push_riff_chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(id);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() & 1 == 1 {
            out.push(0);
        }
    }

    fn wav_fmt_body(channels: u16, sample_rate: u32, byte_rate: u32, bit_depth: u16) -> [u8; 16] {
        let mut body = [0u8; 16];
        body[..2].copy_from_slice(&1u16.to_le_bytes());
        body[2..4].copy_from_slice(&channels.to_le_bytes());
        body[4..8].copy_from_slice(&sample_rate.to_le_bytes());
        body[8..12].copy_from_slice(&byte_rate.to_le_bytes());
        body[12..14].copy_from_slice(&(channels * bit_depth / 8).to_le_bytes());
        body[14..16].copy_from_slice(&bit_depth.to_le_bytes());
        body
    }

    fn iso_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + payload.len());
        out.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    fn iso_box_64(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + payload.len());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(&((16 + payload.len()) as u64).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn wav_parser_handles_valid_padded_and_malformed_chunks() {
        let mut wav = b"RIFF\x00\x00\x00\x00WAVE".to_vec();
        // Odd-length JUNK chunk must align by 1 padding byte before fmt.
        push_riff_chunk(&mut wav, b"JUNK", b"abcde");
        push_riff_chunk(&mut wav, b"fmt ", &wav_fmt_body(2, 48_000, 192_000, 16));
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&384_000u32.to_le_bytes());
        wav.extend_from_slice(&[0u8; 16]);
        assert_eq!(
            probe_audio_facts(Some(wav.len() as u64), ProbeHint::default(), &wav),
            AudioFacts {
                format: AudioKind::Wav,
                tags: Default::default(),
                duration: Some(Duration::from_secs(2)),
                sample_rate: Some(48_000),
                channels: Some(2),
                bit_depth: Some(16),
                bitrate: Some(1_536_000),
            }
        );

        // data chunk before fmt still computes duration.
        let mut reordered = b"RIFF\x00\x00\x00\x00WAVE".to_vec();
        push_riff_chunk(&mut reordered, b"data", &[0u8; 96_000]);
        push_riff_chunk(
            &mut reordered,
            b"fmt ",
            &wav_fmt_body(1, 48_000, 96_000, 16),
        );
        assert_eq!(
            probe_audio_facts(None, ProbeHint::default(), &reordered).duration,
            Some(Duration::from_secs(1))
        );

        // Zero byte_rate avoids division by zero and yields duration: None.
        let mut zero_rate = b"RIFF\x00\x00\x00\x00WAVE".to_vec();
        push_riff_chunk(&mut zero_rate, b"fmt ", &wav_fmt_body(2, 44_100, 0, 16));
        push_riff_chunk(&mut zero_rate, b"data", &[0u8; 64]);
        let facts = probe_audio_facts(None, ProbeHint::default(), &zero_rate);
        assert_eq!(facts.duration, None);
        assert_eq!(facts.bitrate, Some(0));

        // Truncated fmt chunk and u32::MAX JUNK length stop cleanly without panicking.
        let mut truncated_fmt = b"RIFF\x00\x00\x00\x00WAVE".to_vec();
        push_riff_chunk(&mut truncated_fmt, b"fmt ", &[1, 0, 2, 0]);
        let facts = probe_audio_facts(None, ProbeHint::default(), &truncated_fmt);
        assert_eq!(facts.channels, Some(2));
        assert_eq!(facts.sample_rate, None);
        assert_eq!(facts.duration, None);

        let mut huge_chunk = b"RIFF\x00\x00\x00\x00WAVEJUNK".to_vec();
        huge_chunk.extend_from_slice(&u32::MAX.to_le_bytes());
        huge_chunk.extend_from_slice(&[0u8; 8]);
        assert_eq!(
            probe_audio_facts(None, ProbeHint::default(), &huge_chunk),
            AudioFacts {
                format: AudioKind::Wav,
                tags: Default::default(),
                duration: None,
                sample_rate: None,
                channels: None,
                bit_depth: None,
                bitrate: None,
            }
        );

        assert_eq!(
            probe_file(None, ProbeHint::default(), b"RIFF\x24\x00\x00\x00WAV"),
            Metadata::File(FileFacts::Unknown)
        );
    }

    #[test]
    fn flac_parser_extracts_streaminfo_and_guards_zero_and_truncation() {
        let build_flac = |block_header: [u8; 4],
                          sample_rate: u32,
                          channels: u16,
                          bit_depth: u16,
                          total_samples: u64| {
            let mut flac = b"fLaC".to_vec();
            flac.extend_from_slice(&block_header);
            let mut streaminfo = [0u8; 34];
            let packed = (u64::from(sample_rate & 0x000f_ffff) << 44)
                | (u64::from((channels - 1) & 0x7) << 41)
                | (u64::from((bit_depth - 1) & 0x1f) << 36)
                | (total_samples & 0x0000_000f_ffff_ffff);
            streaminfo[10..18].copy_from_slice(&packed.to_be_bytes());
            flac.extend_from_slice(&streaminfo);
            flac
        };

        let valid = build_flac([0x80, 0, 0, 34], 44_100, 2, 16, 88_200);
        assert_eq!(
            probe_audio_facts(Some(88_200), ProbeHint::default(), &valid),
            AudioFacts {
                format: AudioKind::Flac,
                tags: Default::default(),
                duration: Some(Duration::from_secs(2)),
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: Some(16),
                bitrate: Some(352_800),
            }
        );

        // Zero sample_rate avoids division by zero; zero total_samples leaves duration None.
        let zero_rate = build_flac([0x80, 0, 0, 34], 0, 2, 24, 88_200);
        let facts = probe_audio_facts(Some(10_000), ProbeHint::default(), &zero_rate);
        assert_eq!(facts.sample_rate, None);
        assert_eq!(facts.duration, None);
        assert_eq!(facts.bitrate, None);
        assert_eq!(facts.channels, Some(2));
        assert_eq!(facts.bit_depth, Some(24));

        let zero_samples = build_flac([0x80, 0, 0, 34], 48_000, 1, 16, 0);
        let facts = probe_audio_facts(Some(10_000), ProbeHint::default(), &zero_samples);
        assert_eq!(facts.sample_rate, Some(48_000));
        assert_eq!(facts.duration, None);

        // Wrong block type or short STREAMINFO length returns bare FLAC metadata.
        let wrong_type = build_flac([0x84, 0, 0, 34], 44_100, 2, 16, 44_100);
        assert_eq!(
            probe_audio_facts(None, ProbeHint::default(), &wrong_type).sample_rate,
            None
        );
        let short_len = build_flac([0x80, 0, 0, 33], 44_100, 2, 16, 44_100);
        assert_eq!(
            probe_audio_facts(None, ProbeHint::default(), &short_len).sample_rate,
            None
        );

        // Truncated STREAMINFO (< 42 bytes) fails cleanly.
        assert_eq!(
            probe_file(None, ProbeHint::default(), &valid[..41]),
            Metadata::File(FileFacts::Unknown)
        );
    }

    #[test]
    fn ogg_parser_handles_vorbis_opus_and_truncated_pages() {
        let build_ogg = |packet: &[u8]| {
            let mut page = Vec::with_capacity(28 + packet.len());
            page.extend_from_slice(b"OggS");
            page.extend_from_slice(&[0u8; 22]);
            page.push(1);
            page.push(packet.len() as u8);
            page.extend_from_slice(packet);
            page
        };

        let mut opus_packet = b"OpusHead".to_vec();
        opus_packet.extend_from_slice(&[1, 2, 0x38, 0x01, 0x80, 0xbb, 0, 0, 0, 0, 0]);
        assert_eq!(
            probe_audio_facts(None, ProbeHint::default(), &build_ogg(&opus_packet)),
            AudioFacts {
                format: AudioKind::Opus,
                tags: Default::default(),
                duration: None,
                sample_rate: Some(48_000),
                channels: Some(2),
                bit_depth: None,
                bitrate: None,
            }
        );

        // Truncated OpusHead without channel byte still yields Opus at 48 kHz.
        assert_eq!(
            probe_audio_facts(None, ProbeHint::default(), &build_ogg(b"OpusHead")).channels,
            None
        );

        let build_vorbis = |channels: u8, sample_rate: u32, nominal_bitrate: i32| {
            let mut pkt = b"\x01vorbis".to_vec();
            pkt.extend_from_slice(&0u32.to_le_bytes());
            pkt.push(channels);
            pkt.extend_from_slice(&sample_rate.to_le_bytes());
            pkt.extend_from_slice(&0i32.to_le_bytes());
            pkt.extend_from_slice(&nominal_bitrate.to_le_bytes());
            pkt.extend_from_slice(&0i32.to_le_bytes());
            pkt.extend_from_slice(&[0x88, 1]);
            build_ogg(&pkt)
        };
        assert_eq!(
            probe_audio_facts(
                None,
                ProbeHint::default(),
                &build_vorbis(2, 44_100, 128_000)
            ),
            AudioFacts {
                format: AudioKind::OggVorbis,
                tags: Default::default(),
                duration: None,
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: None,
                bitrate: Some(128_000),
            }
        );

        // Zero sample rate and negative nominal bitrate are filtered out.
        let invalid_vorbis = build_vorbis(2, 0, -1);
        let facts = probe_audio_facts(None, ProbeHint::default(), &invalid_vorbis);
        assert_eq!(facts.sample_rate, None);
        assert_eq!(facts.bitrate, None);
        assert_eq!(facts.channels, Some(2));

        // Truncated packet body keeps partial fields, while truncated header / unknown codec yields Unknown.
        let valid_vorbis = build_vorbis(2, 48_000, 192_000);
        let partial = probe_audio_facts(None, ProbeHint::default(), &valid_vorbis[..40]);
        assert_eq!(partial.format, AudioKind::OggVorbis);
        assert_eq!(partial.channels, Some(2));
        assert_eq!(partial.sample_rate, None);

        assert_eq!(
            probe_file(None, ProbeHint::default(), &valid_vorbis[..26]),
            Metadata::File(FileFacts::Unknown)
        );
        assert_eq!(
            probe_file(None, ProbeHint::default(), &build_ogg(b"\x80theora")),
            Metadata::File(FileFacts::Unknown)
        );
    }

    #[test]
    fn mp3_parser_handles_id3v2_xing_cbr_and_invalid_headers() {
        let audio_hint = ProbeHint {
            audio: true,
            text: false,
        };
        // MPEG-1 Layer III, 128 kbps (idx 9), 44100 Hz (idx 0), stereo (mode 0, side_info = 32).
        let frame_header = [0xff, 0xfb, 0x90, 0x00];

        // CBR frame without Xing/Info header: recognized only when hint.audio is true.
        assert_eq!(
            probe_file(Some(64_000), ProbeHint::default(), &frame_header),
            Metadata::File(FileFacts::Unknown)
        );
        assert_eq!(
            probe_audio_facts(Some(64_000), audio_hint, &frame_header),
            AudioFacts {
                format: AudioKind::Mp3,
                tags: Default::default(),
                duration: None,
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: None,
                bitrate: Some(128_000),
            }
        );

        // ID3v2 (128-byte syncsafe body) + Xing VBR tag (100 frames).
        let mut xing = b"ID3\x03\x00\x00\x00\x00\x01\x00".to_vec();
        xing.extend_from_slice(&[0u8; 128]);
        xing.extend_from_slice(&frame_header);
        xing.extend_from_slice(&[0u8; 32]);
        xing.extend_from_slice(b"Xing");
        xing.extend_from_slice(&1u32.to_be_bytes());
        xing.extend_from_slice(&100u32.to_be_bytes());
        let expected_duration = Duration::new(2, 612_244_897);
        let expected_bitrate = (200_000f64 * 8.0 / expected_duration.as_secs_f64()) as u64;
        assert_eq!(
            probe_audio_facts(Some(200_000), audio_hint, &xing),
            AudioFacts {
                format: AudioKind::Mp3,
                tags: Default::default(),
                duration: Some(expected_duration),
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: None,
                bitrate: Some(expected_bitrate),
            }
        );

        // Mono Info tag with frames = 0 falls back to frame bitrate and duration: None.
        let mut zero_info = vec![0xff, 0xfb, 0x90, 0xc0];
        zero_info.extend_from_slice(&[0u8; 17]);
        zero_info.extend_from_slice(b"Info");
        zero_info.extend_from_slice(&1u32.to_be_bytes());
        zero_info.extend_from_slice(&0u32.to_be_bytes());
        let facts = probe_audio_facts(Some(200_000), audio_hint, &zero_info);
        assert_eq!(facts.channels, Some(1));
        assert_eq!(facts.duration, None);
        assert_eq!(facts.bitrate, Some(128_000));

        // Malformed sync headers and ID3v2 sizes extending past the prefix fail cleanly.
        for bad in [
            [0xff, 0xeb, 0x90, 0x00].as_slice(), // reserved MPEG version 1
            &[0xff, 0xf9, 0x90, 0x00],           // reserved layer 0
            &[0xff, 0xfb, 0x00, 0x00],           // free bitrate index 0
            &[0xff, 0xfb, 0xf0, 0x00],           // invalid bitrate index 15
            &[0xff, 0xfb, 0x9c, 0x00],           // reserved sample rate index 3
            b"ID3\x03\x00\x00\x7f\x7f\x7f\x7f",  // syncsafe tag larger than slice
            b"ID3\x03\x00",                      // truncated ID3v2 header
        ] {
            assert_eq!(
                probe_file(None, audio_hint, bad),
                Metadata::File(FileFacts::Unknown)
            );
        }
    }

    #[test]
    fn iso_bmff_parser_handles_video_audio_box_variants_and_malformed_boxes() {
        let mvhd_v0 = |timescale: u32, duration: u32| {
            let mut payload = vec![0u8; 100];
            payload[0] = 0;
            payload[12..16].copy_from_slice(&timescale.to_be_bytes());
            payload[16..20].copy_from_slice(&duration.to_be_bytes());
            iso_box(b"mvhd", &payload)
        };
        let tkhd_v0 = |width: u32, height: u32| {
            let mut payload = vec![0u8; 84];
            payload[0] = 0;
            payload[76..80].copy_from_slice(&(width << 16).to_be_bytes());
            payload[80..84].copy_from_slice(&(height << 16).to_be_bytes());
            iso_box(b"tkhd", &payload)
        };
        let hdlr = |handler: &[u8; 4]| {
            let mut payload = vec![0u8; 24];
            payload[8..12].copy_from_slice(handler);
            iso_box(b"hdlr", &payload)
        };
        let stsd = |entry: Vec<u8>| {
            let mut payload = vec![0u8; 8];
            payload[7] = 1;
            payload.extend_from_slice(&entry);
            iso_box(b"stsd", &payload)
        };
        let visual_entry = |fourcc: &[u8; 4], width: u16, height: u16| {
            let mut payload = vec![0u8; 78];
            payload[24..26].copy_from_slice(&width.to_be_bytes());
            payload[26..28].copy_from_slice(&height.to_be_bytes());
            iso_box(fourcc, &payload)
        };
        let audio_entry = |fourcc: &[u8; 4], channels: u16, bits: u16, rate: u32| {
            let mut payload = vec![0u8; 28];
            payload[16..18].copy_from_slice(&channels.to_be_bytes());
            payload[18..20].copy_from_slice(&bits.to_be_bytes());
            payload[24..28].copy_from_slice(&(rate << 16).to_be_bytes());
            iso_box(fourcc, &payload)
        };

        let mdhd_v0 = |timescale: u32, duration: u32| {
            let mut payload = vec![0u8; 24];
            payload[0] = 0;
            payload[12..16].copy_from_slice(&timescale.to_be_bytes());
            payload[16..20].copy_from_slice(&duration.to_be_bytes());
            iso_box(b"mdhd", &payload)
        };

        // Valid MP4 video: track mdhd + stsd overrides mvhd + tkhd.
        let mdia_video = [
            mdhd_v0(90_000, 180_000),
            hdlr(b"vide"),
            iso_box(
                b"minf",
                &iso_box(b"stbl", &stsd(visual_entry(b"avc1", 1920, 1080))),
            ),
        ]
        .concat();
        let trak_video = iso_box(
            b"trak",
            &[tkhd_v0(1280, 720), iso_box(b"mdia", &mdia_video)].concat(),
        );
        let mp4 = [
            iso_box(b"ftyp", b"isom\x00\x00\x02\x00isom"),
            iso_box(b"moov", &[mvhd_v0(1000, 5000), trak_video].concat()),
        ]
        .concat();
        assert_eq!(
            probe_video_facts(&mp4),
            VideoFacts {
                format: VideoKind::Mp4,
                duration: Some(Duration::from_secs(2)),
                dimensions: PixelSize::new(1920, 1080),
                codec: Some("H.264".into()),
            }
        );

        // QuickTime with 64-bit box size, v1 mvhd/tkhd, and tkhd dimensions fallback for ProRes.
        let mut mvhd_v1 = vec![0u8; 112];
        mvhd_v1[0] = 1;
        mvhd_v1[20..24].copy_from_slice(&1000u32.to_be_bytes());
        mvhd_v1[24..32].copy_from_slice(&4000u64.to_be_bytes());
        let mut tkhd_v1 = vec![0u8; 96];
        tkhd_v1[0] = 1;
        tkhd_v1[88..92].copy_from_slice(&(640u32 << 16).to_be_bytes());
        tkhd_v1[92..96].copy_from_slice(&(480u32 << 16).to_be_bytes());
        let prores_entry = iso_box(b"apch", &[0u8; 32]);
        let qt_trak = iso_box(
            b"trak",
            &[
                iso_box(b"tkhd", &tkhd_v1),
                iso_box(
                    b"mdia",
                    &[
                        hdlr(b"vide"),
                        iso_box(b"minf", &iso_box(b"stbl", &stsd(prores_entry))),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        );
        let qt = [
            iso_box_64(b"ftyp", b"qt  \x00\x00\x00\x00"),
            iso_box_64(b"moov", &[iso_box(b"mvhd", &mvhd_v1), qt_trak].concat()),
        ]
        .concat();
        assert_eq!(
            probe_video_facts(&qt),
            VideoFacts {
                format: VideoKind::QuickTime,
                duration: Some(Duration::from_secs(4)),
                dimensions: PixelSize::new(640, 480),
                codec: Some("ProRes".into()),
            }
        );

        // Valid M4A audio with size=0 trailing moov box.
        let m4a_moov_body = iso_box(
            b"trak",
            &iso_box(
                b"mdia",
                &[
                    mdhd_v0(44_100, 88_200),
                    hdlr(b"soun"),
                    iso_box(
                        b"minf",
                        &iso_box(b"stbl", &stsd(audio_entry(b"mp4a", 2, 16, 44_100))),
                    ),
                ]
                .concat(),
            ),
        );
        let mut m4a = iso_box(b"ftyp", b"M4A \x00\x00\x00\x00");
        m4a.extend_from_slice(&0u32.to_be_bytes()); // size == 0 means "to end of buffer"
        m4a.extend_from_slice(b"moov");
        m4a.extend_from_slice(&m4a_moov_body);
        assert_eq!(
            probe_audio_facts(Some(64_000), ProbeHint::default(), &m4a),
            AudioFacts {
                format: AudioKind::M4a,
                tags: Default::default(),
                duration: Some(Duration::from_secs(2)),
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: Some(16),
                bitrate: Some(256_000),
            }
        );

        // M4A brand without moov in prefix still classifies as M4a; 3GPP brand classifies as ThreeGpp.
        let mut m4a_mdat_only = iso_box(b"ftyp", b"M4A ");
        m4a_mdat_only.extend_from_slice(&1024u32.to_be_bytes());
        m4a_mdat_only.extend_from_slice(b"mdat");
        assert_eq!(
            probe_audio_facts(None, ProbeHint::default(), &m4a_mdat_only).format,
            AudioKind::M4a
        );
        assert_eq!(
            probe_video_facts(&iso_box(b"ftyp", b"3gp4")).format,
            VideoKind::ThreeGpp
        );

        // Zero timescale, zero dimensions, and overflow/undersized boxes do not divide by zero or panic.
        let zero_scale_mp4 = [
            iso_box(b"ftyp", b"isom"),
            iso_box(
                b"moov",
                &[
                    mvhd_v0(0, 5000),
                    iso_box(
                        b"trak",
                        &[tkhd_v0(0, 0), iso_box(b"mdia", &[hdlr(b"vide")].concat())].concat(),
                    ),
                ]
                .concat(),
            ),
        ]
        .concat();
        assert_eq!(
            probe_video_facts(&zero_scale_mp4),
            VideoFacts {
                format: VideoKind::Mp4,
                duration: None,
                dimensions: None,
                codec: None,
            }
        );

        for bad_box in [
            [0, 0, 0, 4, b'f', b'r', b'e', b'e'].as_slice(), // size < 8
            &[0, 0, 0, 1, b'f', b'r', b'e', b'e', 0, 0, 0, 0, 0, 0, 0, 10], // large size < 16
            &[
                0, 0, 0, 1, b'f', b'r', b'e', b'e', 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            ], // u64::MAX large size overflow
        ] {
            let mut bytes = iso_box(b"ftyp", b"isom");
            bytes.extend_from_slice(bad_box);
            assert_eq!(probe_video_facts(&bytes).format, VideoKind::Mp4);
        }
    }

    #[test]
    fn ebml_and_avi_parsers_classify_containers_and_bounds() {
        assert_eq!(
            probe_video_facts(b"\x1aE\xdf\xa3\x9fB\x82\x84webm"),
            VideoFacts {
                format: VideoKind::WebM,
                duration: None,
                dimensions: None,
                codec: None,
            }
        );
        assert_eq!(
            probe_video_facts(b"\x1aE\xdf\xa3\x9fB\x82\x88matroska").format,
            VideoKind::Matroska
        );
        assert_eq!(
            probe_video_facts(b"\x1aE\xdf\xa3").format,
            VideoKind::Matroska
        );

        // DocType beyond the 4096-byte head scan stays Matroska.
        let mut late_webm = b"\x1aE\xdf\xa3".to_vec();
        late_webm.resize(4100, 0);
        late_webm.extend_from_slice(b"webm");
        assert_eq!(probe_video_facts(&late_webm).format, VideoKind::Matroska);

        assert_eq!(
            probe_file(None, ProbeHint::default(), b"\x1aE\xdf"),
            Metadata::File(FileFacts::Unknown)
        );
        assert_eq!(
            probe_video_facts(b"RIFF\x00\x00\x00\x00AVI "),
            VideoFacts {
                format: VideoKind::Avi,
                duration: None,
                dimensions: None,
                codec: None,
            }
        );
        assert_eq!(
            probe_file(None, ProbeHint::default(), b"RIFF\x00\x00\x00\x00AVI"),
            Metadata::File(FileFacts::Unknown)
        );
    }

    fn mp4_with_mvhd_v1(body_len: usize, timescale: u32, duration: u64) -> Vec<u8> {
        let mut mvhd = vec![0u8; body_len];
        mvhd[0] = 1;
        mvhd[20..24].copy_from_slice(&timescale.to_be_bytes());
        mvhd[24..32].copy_from_slice(&duration.to_be_bytes());
        [
            iso_box(b"ftyp", b"isom\x00\x00\x02\x00isom"),
            iso_box(b"moov", &iso_box(b"mvhd", &mvhd)),
        ]
        .concat()
    }

    fn mp4_with_mvhd_v0(timescale: u32, duration: u32) -> Vec<u8> {
        let mut mvhd = vec![0u8; 100];
        mvhd[0] = 0;
        mvhd[12..16].copy_from_slice(&timescale.to_be_bytes());
        mvhd[16..20].copy_from_slice(&duration.to_be_bytes());
        [
            iso_box(b"ftyp", b"isom\x00\x00\x02\x00isom"),
            iso_box(b"moov", &iso_box(b"mvhd", &mvhd)),
        ]
        .concat()
    }

    #[test]
    fn iso_bmff_duration_handles_sentinels_large_u64_and_truncated_mvhd() {
        assert_eq!(
            probe_video_facts(&mp4_with_mvhd_v0(1_000, u32::MAX)).duration,
            None
        );
        assert_eq!(
            probe_video_facts(&mp4_with_mvhd_v1(112, 1, u64::MAX)).duration,
            None
        );
        assert_eq!(
            probe_video_facts(&mp4_with_mvhd_v1(112, 1, u64::MAX - 1)).duration,
            Some(Duration::from_secs(u64::MAX - 1))
        );
        assert_eq!(
            probe_video_facts(&mp4_with_mvhd_v1(32, 1, u64::MAX)).duration,
            None
        );
    }

    #[test]
    fn probe_path_seeks_past_large_mdat_to_trailing_moov() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trailing-moov.mp4");

        let mut tkhd = vec![0u8; 84];
        tkhd[76..80].copy_from_slice(&(1920u32 << 16).to_be_bytes());
        tkhd[80..84].copy_from_slice(&(1080u32 << 16).to_be_bytes());

        let mut mdhd = vec![0u8; 24];
        mdhd[12..16].copy_from_slice(&90_000u32.to_be_bytes());
        mdhd[16..20].copy_from_slice(&270_000u32.to_be_bytes());

        let mut hdlr = vec![0u8; 24];
        hdlr[8..12].copy_from_slice(b"vide");

        let mut visual = vec![0u8; 78];
        visual[24..26].copy_from_slice(&1920u16.to_be_bytes());
        visual[26..28].copy_from_slice(&1080u16.to_be_bytes());
        let mut stsd = vec![0u8; 8];
        stsd[7] = 1;
        stsd.extend_from_slice(&iso_box(b"avc1", &visual));

        let trak = iso_box(
            b"trak",
            &[
                iso_box(b"tkhd", &tkhd),
                iso_box(
                    b"mdia",
                    &[
                        iso_box(b"mdhd", &mdhd),
                        iso_box(b"hdlr", &hdlr),
                        iso_box(b"minf", &iso_box(b"stbl", &iso_box(b"stsd", &stsd))),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        );
        let bytes = [
            iso_box(b"ftyp", b"mp42\x00\x00\x00\x00mp42isom"),
            iso_box(b"mdat", &vec![0u8; PREFIX_BYTES * 2]),
            iso_box(b"moov", &trak),
        ]
        .concat();
        fs::write(&path, &bytes).expect("write mp4");

        let metadata =
            probe_path(&path, Some(bytes.len() as u64), ProbeHint::default()).expect("probe_path");
        assert_eq!(
            metadata,
            Metadata::File(FileFacts::Video(VideoFacts {
                format: VideoKind::Mp4,
                duration: Some(Duration::from_secs(3)),
                dimensions: PixelSize::new(1920, 1080),
                codec: Some("H.264".into()),
            }))
        );
    }
}
