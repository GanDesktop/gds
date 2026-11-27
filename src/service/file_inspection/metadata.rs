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
    (channels > 0 && bits.is_multiple_of(channels)).then_some((bits / channels) as u8)
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
    bytes.get(table_end..end.min(bytes.len()))
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

fn next_box<R: Read + Seek>(
    reader: &mut R,
    cursor: &mut u64,
    container_end: u64,
) -> Option<BoxSpan> {
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
        b"avc1" | b"avc3" | b"hvc1" | b"hev1" | b"av01" | b"vp09" | b"mp4v" | b"apch" | b"apcn"
        | b"apcs" | b"apco" | b"ap4h" => {
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
mod tests;
