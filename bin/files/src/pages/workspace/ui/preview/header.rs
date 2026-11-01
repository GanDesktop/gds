//! Bounded, header-only metadata probes for the preview panel.
//!
//! Every probe reads at most [`PREFIX_BYTES`] from the start of a regular file.
//! Thumbnail decoding runs independently in the shared thumbnail service.
//! Parsers never walk past their slice: a header that the prefix cut short
//! simply yields fewer fields instead of a failure.
//!
//! Fields that need the end of a file (MP4 `moov` after `mdat`, Ogg granule of
//! the last page, PDF trailers, ZIP central directories) stay `None`. The panel
//! shows only what the prefix really contains.

use std::{
    fs::{self, File},
    io::{self, Cursor, Read},
    path::{Path, PathBuf},
    time::Duration,
};

use image::{ExtendedColorType, ImageDecoder, ImageReader};

/// Bytes sampled from a file for format and header metadata.
pub(super) const PREFIX_BYTES: usize = 256 * 1024;

/// Direct children inspected for a folder; counting stops at this bound.
const DIRECTORY_CHILD_LIMIT: usize = 256;

/// One probe result for the selected entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Probe {
    File(FileFacts),
    Directory(DirectoryFacts),
    Symlink(SymlinkFacts),
}

/// Facts learned from the first bytes of a regular file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FileFacts {
    Image(ImageFacts),
    Audio(AudioFacts),
    Video(VideoFacts),
    Text(TextFacts),
    /// Bytes were read but no supported structure was recognised.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ImageFacts {
    pub format: ImageKind,
    pub size: Option<PixelSize>,
    pub bit_depth: Option<u8>,
    /// Only reported for formats whose header states transparency structurally.
    pub alpha: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PixelSize {
    pub width: u32,
    pub height: u32,
}

impl PixelSize {
    fn new(width: u32, height: u32) -> Option<Self> {
        (width > 0 && height > 0).then_some(Self { width, height })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ImageKind {
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

    pub(super) fn label(self) -> &'static str {
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
pub(super) struct AudioFacts {
    pub format: AudioKind,
    pub duration: Option<Duration>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub bit_depth: Option<u16>,
    pub bitrate: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AudioKind {
    Wav,
    Flac,
    OggVorbis,
    Opus,
    Mp3,
    M4a,
}

impl AudioKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::OggVorbis => "Ogg Vorbis",
            Self::Opus => "Opus",
            Self::Mp3 => "MP3",
            Self::M4a => "M4A",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct VideoFacts {
    pub format: VideoKind,
    pub duration: Option<Duration>,
    pub dimensions: Option<PixelSize>,
    pub codec: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VideoKind {
    Mp4,
    QuickTime,
    ThreeGpp,
    Avi,
    WebM,
    Matroska,
}

impl VideoKind {
    pub(super) fn label(self) -> &'static str {
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
pub(super) struct TextFacts {
    pub encoding: TextEncoding,
    pub line_endings: Option<LineEndings>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TextEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

impl TextEncoding {
    pub(super) fn label(self) -> &'static str {
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
pub(super) enum LineEndings {
    Lf,
    Crlf,
    Cr,
    Mixed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DirectoryFacts {
    pub folders: u32,
    pub files: u32,
    pub links: u32,
    /// The directory holds more children than [`DIRECTORY_CHILD_LIMIT`].
    pub capped: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SymlinkFacts {
    pub target: PathBuf,
}

/// Read at most `limit` bytes from the start of a regular file.
///
/// Directories, FIFOs and device nodes are refused before opening so a preview
/// never blocks on a special file.
///
/// # Errors
/// Returns the I/O error when the path is not a regular file or cannot be read.
pub(super) fn read_prefix(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len().min(limit as u64) as usize);
    file.take(limit as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
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

/// Inspect the prefix of a regular file.
///
/// `hint` comes from the directory listing's content category and keeps
/// arbitrary binaries from being presented as text or MP3.
pub(super) fn probe_file(size: Option<u64>, hint: ProbeHint, bytes: &[u8]) -> Probe {
    if bytes.is_empty() {
        return Probe::File(FileFacts::Unknown);
    }
    if let Ok(format) = image::guess_format(bytes)
        && let Some(image) = image_facts(format, bytes)
    {
        return Probe::File(FileFacts::Image(image));
    }
    if let Some(audio) = audio_facts(bytes, size, hint.audio) {
        return Probe::File(FileFacts::Audio(audio));
    }
    if let Some(video) = video_facts(bytes, size) {
        return Probe::File(FileFacts::Video(video));
    }
    if hint.text
        && let Some(text) = text_facts(bytes, bytes.len() == PREFIX_BYTES)
    {
        return Probe::File(FileFacts::Text(text));
    }
    Probe::File(FileFacts::Unknown)
}

/// Count one level of direct children, never recursing.
pub(super) fn probe_directory(path: &Path) -> Probe {
    let mut facts = DirectoryFacts::default();
    let Ok(entries) = fs::read_dir(path) else {
        return Probe::Directory(facts);
    };
    let mut entries = entries.flatten();
    for entry in entries.by_ref().take(DIRECTORY_CHILD_LIMIT) {
        match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => facts.folders += 1,
            Ok(file_type) if file_type.is_symlink() => facts.links += 1,
            Ok(_) => facts.files += 1,
            Err(_) => {}
        }
    }
    facts.capped = entries.next().is_some();
    Probe::Directory(facts)
}

/// Read the stored link text; the target's state comes from the listing.
pub(super) fn probe_symlink(path: &Path) -> Option<Probe> {
    fs::read_link(path)
        .ok()
        .map(|target| Probe::Symlink(SymlinkFacts { target }))
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
        apply_png_header(&mut facts, bytes);
    }
    if let Ok(decoder) = ImageReader::with_format(Cursor::new(bytes), format).into_decoder() {
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
fn apply_png_header(facts: &mut ImageFacts, bytes: &[u8]) {
    let Some(ihdr) = png_ihdr(bytes) else {
        return;
    };
    facts.size = PixelSize::new(ihdr.width, ihdr.height);
    facts.bit_depth = Some(ihdr.bit_depth);
    facts.alpha = match ihdr.color_type {
        4 | 6 => Some(true),
        3 => png_has_trns(bytes),
        0 | 2 => Some(false),
        _ => None,
    };
}

struct PngHeader {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
}

fn png_ihdr(bytes: &[u8]) -> Option<PngHeader> {
    const SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.get(..8)? != SIGNATURE || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32_be(bytes, 16)?;
    let height = u32_be(bytes, 20)?;
    Some(PngHeader {
        width,
        height,
        bit_depth: *bytes.get(24)?,
        color_type: *bytes.get(25)?,
    })
}

/// Palette transparency is an optional `tRNS` chunk before the first `IDAT`.
/// `None` means the prefix ended before the answer was available.
fn png_has_trns(bytes: &[u8]) -> Option<bool> {
    let mut offset = 8;
    while let Some(header) = bytes.get(offset..offset + 8) {
        let length = u32_be(header, 0)? as usize;
        let kind = &header[4..8];
        if kind == b"tRNS" {
            return Some(true);
        }
        if kind == b"IDAT" || kind == b"IEND" {
            return Some(false);
        }
        offset = offset.checked_add(12 + length)?;
    }
    None
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

fn audio_facts(bytes: &[u8], size: Option<u64>, mp3_allowed: bool) -> Option<AudioFacts> {
    if let Some(facts) = wav_facts(bytes) {
        return Some(facts);
    }
    if let Some(facts) = flac_facts(bytes, size) {
        return Some(facts);
    }
    if let Some(facts) = ogg_facts(bytes) {
        return Some(facts);
    }
    if let Some(iso) = iso_facts(bytes)
        && iso_is_audio(iso.brand, &iso.info)
    {
        return Some(audio_from_iso(&iso.info, size));
    }
    if mp3_allowed && let Some(facts) = mp3_facts(bytes, size) {
        return Some(facts);
    }
    None
}

fn wav_facts(bytes: &[u8]) -> Option<AudioFacts> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut facts = AudioFacts {
        format: AudioKind::Wav,
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
    if let (Some(data_size), Some(byte_rate)) = (data_size, byte_rate) {
        if byte_rate > 0 {
            facts.duration = Some(Duration::from_secs_f64(
                data_size as f64 / f64::from(byte_rate),
            ));
        }
    }
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
    let duration = (sample_rate > 0 && total_samples > 0)
        .then(|| Duration::from_secs_f64(total_samples as f64 / f64::from(sample_rate)));
    Some(AudioFacts {
        format: AudioKind::Flac,
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
    (frames > 0 && frame.sample_rate > 0).then(|| {
        Duration::from_secs_f64(
            f64::from(frames) * f64::from(frame.samples_per_frame) / f64::from(frame.sample_rate),
        )
    })
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

struct IsoBox<'a> {
    kind: [u8; 4],
    payload: &'a [u8],
}

struct IsoBoxes<'a> {
    data: &'a [u8],
}

impl<'a> IsoBoxes<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
}

impl<'a> Iterator for IsoBoxes<'a> {
    type Item = IsoBox<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let header = self.data.get(..8)?;
        let kind: [u8; 4] = header[4..8].try_into().ok()?;
        let size32 = u32::from_be_bytes(header[..4].try_into().ok()?);
        let (header_len, size) = match size32 {
            0 => (8usize, self.data.len()),
            1 => {
                let large = u64::from_be_bytes(self.data.get(8..16)?.try_into().ok()?);
                let size = usize::try_from(large).ok()?;
                (16usize, size)
            }
            size => (8usize, size as usize),
        };
        if size < header_len || size > self.data.len() {
            self.data = &[];
            return None;
        }
        let payload = &self.data[header_len..size];
        self.data = &self.data[size..];
        Some(IsoBox { kind, payload })
    }
}

fn iso_facts(bytes: &[u8]) -> Option<IsoFile> {
    let mut major = None;
    let mut info = IsoInfo::default();
    for block in IsoBoxes::new(bytes) {
        match &block.kind {
            b"ftyp" if major.is_none() => {
                major = block
                    .payload
                    .get(..4)
                    .and_then(|brand| brand.try_into().ok());
            }
            b"moov" => read_moov(block.payload, &mut info),
            _ => {}
        }
    }
    Some(IsoFile {
        brand: major?,
        info,
    })
}

fn read_moov(payload: &[u8], info: &mut IsoInfo) {
    for block in IsoBoxes::new(payload) {
        match &block.kind {
            b"mvhd" => {
                let header = media_header(block.payload);
                info.movie_timescale = header.timescale;
                info.movie_duration = header.duration;
            }
            b"trak" => info.tracks.push(read_trak(block.payload)),
            _ => {}
        }
    }
}

fn read_trak(payload: &[u8]) -> IsoTrack {
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
    for block in IsoBoxes::new(payload) {
        match &block.kind {
            b"tkhd" => track.dimensions = track_dimensions(block.payload),
            b"mdia" => read_mdia(block.payload, &mut track),
            _ => {}
        }
    }
    track
}

fn read_mdia(payload: &[u8], track: &mut IsoTrack) {
    for block in IsoBoxes::new(payload) {
        match &block.kind {
            b"mdhd" => {
                let header = media_header(block.payload);
                track.timescale = header.timescale;
                track.duration = header.duration;
            }
            b"hdlr" => {
                track.handler = block
                    .payload
                    .get(8..12)
                    .and_then(|kind| kind.try_into().ok());
            }
            b"minf" => {
                for inner in IsoBoxes::new(block.payload) {
                    if inner.kind == *b"stbl" {
                        read_stbl(inner.payload, track);
                    }
                }
            }
            _ => {}
        }
    }
}

fn read_stbl(payload: &[u8], track: &mut IsoTrack) {
    for block in IsoBoxes::new(payload) {
        if block.kind == *b"stsd" {
            read_stsd(block.payload, track);
        }
    }
}

/// Only the first sample entry is inspected; a file with several entries still
/// reports its main track codec.
fn read_stsd(payload: &[u8], track: &mut IsoTrack) {
    let Some(entries) = IsoBoxes::new(payload.get(8..).unwrap_or_default()).next() else {
        return;
    };
    track.codec = Some(entries.kind);
    match &entries.kind {
        // Visual sample entries: width/height follow the 32-byte base fields.
        b"avc1" | b"avc3" | b"hvc1" | b"hev1" | b"av01" | b"vp09" | b"mp4v" => {
            let width = u16_be(entries.payload, 24);
            let height = u16_be(entries.payload, 26);
            track.dimensions = width
                .zip(height)
                .and_then(|(width, height)| PixelSize::new(u32::from(width), u32::from(height)))
                .or(track.dimensions);
        }
        // Audio sample entries: channel count, sample size, 16.16 sample rate.
        b"mp4a" | b"alac" | b"Opus" | b"fLaC" => {
            track.channels = u16_be(entries.payload, 16);
            track.bit_depth = u16_be(entries.payload, 18);
            track.sample_rate = u32_be(entries.payload, 24).map(|rate| rate >> 16);
        }
        _ => {}
    }
}

struct MediaHeader {
    timescale: Option<u32>,
    duration: Option<u64>,
}

fn media_header(payload: &[u8]) -> MediaHeader {
    match payload.first() {
        Some(0) => MediaHeader {
            timescale: u32_be(payload, 12),
            duration: u32_be(payload, 16).map(u64::from),
        },
        Some(1) => MediaHeader {
            timescale: u32_be(payload, 20),
            duration: u64_be(payload, 24),
        },
        _ => MediaHeader {
            timescale: None,
            duration: None,
        },
    }
}

fn track_dimensions(payload: &[u8]) -> Option<PixelSize> {
    let offset = match payload.first()? {
        0 => 76,
        1 => 88,
        _ => return None,
    };
    let width = u32_be(payload, offset)? >> 16;
    let height = u32_be(payload, offset + 4)? >> 16;
    PixelSize::new(width, height)
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
        duration,
        sample_rate: track
            .and_then(|track| track.sample_rate)
            .filter(|rate| *rate > 0),
        channels: track.and_then(|track| track.channels),
        bit_depth: track.and_then(|track| track.bit_depth),
        bitrate: average_bitrate(size, duration),
    }
}

fn video_facts(bytes: &[u8], _size: Option<u64>) -> Option<VideoFacts> {
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
    let iso = iso_facts(bytes)?;
    if iso_is_audio(iso.brand, &iso.info) {
        return None;
    }
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
    Some(VideoFacts {
        format: iso_kind(iso.brand),
        duration,
        dimensions,
        codec,
    })
}

fn duration_of(timescale: Option<u32>, units: Option<u64>) -> Option<Duration> {
    let timescale = timescale.filter(|value| *value > 0)?;
    let units = units.filter(|value| *value > 0)?;
    Some(Duration::from_secs_f64(units as f64 / f64::from(timescale)))
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

fn u16_be(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
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
