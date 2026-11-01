use std::{io::Write, time::Duration};

use super::header::{
    self, AudioKind, FileFacts, ImageKind, LineEndings, PixelSize, Probe, ProbeHint, TextEncoding,
    VideoKind,
};
use super::info;

/// A valid 1x1 grayscale-with-alpha PNG fixture. The point of the regression is
/// that its name lies.
const PNG_BYTES: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5, 0x1c, 0x0c,
    0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0x00, 0x00,
    0x00, 0x06, 0x00, 0x02, 0x30, 0x81, 0xd0, 0x2f, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44,
    0xae, 0x42, 0x60, 0x82,
];

#[test]
fn png_probe_reads_dimensions_depth_and_alpha_from_the_header() {
    let Some(FileFacts::Image(facts)) =
        file_facts(header::probe_file(None, ProbeHint::default(), PNG_BYTES))
    else {
        panic!("PNG header was not recognised as an image");
    };
    assert_eq!(facts.format, ImageKind::Png);
    assert_eq!(
        facts.size,
        Some(PixelSize {
            width: 1,
            height: 1
        })
    );
    assert_eq!(facts.bit_depth, Some(8));
    assert_eq!(facts.alpha, Some(true));
}

#[test]
fn jpeg_probe_keeps_the_format_when_the_header_is_cut_short() {
    let bytes = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10];
    let Some(FileFacts::Image(facts)) =
        file_facts(header::probe_file(None, ProbeHint::default(), &bytes))
    else {
        panic!("JPEG signature was not recognised");
    };
    assert_eq!(facts.format, ImageKind::Jpeg);
    assert_eq!(facts.size, None);
    assert_eq!(facts.alpha, None);
}

#[test]
fn cut_png_header_does_not_panic_or_invent_dimensions() {
    let cut = &PNG_BYTES[..20];
    let Some(FileFacts::Image(facts)) =
        file_facts(header::probe_file(None, ProbeHint::default(), cut))
    else {
        panic!("PNG signature was lost when the header was cut");
    };
    assert_eq!(facts.format, ImageKind::Png);
    assert_eq!(facts.size, None);
}

#[test]
fn wav_header_yields_duration_rate_channels_and_bitrate() {
    let bytes = wav_header(176_400);
    let Some(FileFacts::Audio(facts)) =
        file_facts(header::probe_file(None, ProbeHint::default(), &bytes))
    else {
        panic!("WAV header was not recognised");
    };
    assert_eq!(facts.format, AudioKind::Wav);
    assert_eq!(facts.duration, Some(Duration::from_secs(1)));
    assert_eq!(facts.sample_rate, Some(44_100));
    assert_eq!(facts.channels, Some(2));
    assert_eq!(facts.bit_depth, Some(16));
    assert_eq!(facts.bitrate, Some(1_411_200));
}

#[test]
fn flac_streaminfo_yields_duration_and_sample_format() {
    let bytes = flac_header(44_100, 2, 16, 44_100);
    let Some(FileFacts::Audio(facts)) = file_facts(header::probe_file(
        Some(176_400),
        ProbeHint::default(),
        &bytes,
    )) else {
        panic!("FLAC header was not recognised");
    };
    assert_eq!(facts.format, AudioKind::Flac);
    assert_eq!(facts.duration, Some(Duration::from_secs(1)));
    assert_eq!(facts.sample_rate, Some(44_100));
    assert_eq!(facts.channels, Some(2));
    assert_eq!(facts.bit_depth, Some(16));
    assert_eq!(facts.bitrate, Some(1_411_200));
}

#[test]
fn mp3_frame_is_only_claimed_with_an_audio_hint() {
    let mut bytes = vec![0xff, 0xfb, 0x90, 0x00];
    bytes.resize(100, 0);
    assert!(matches!(
        header::probe_file(None, ProbeHint::default(), &bytes),
        Probe::File(FileFacts::Unknown)
    ));

    let Some(FileFacts::Audio(facts)) = file_facts(header::probe_file(
        None,
        ProbeHint {
            audio: true,
            ..ProbeHint::default()
        },
        &bytes,
    )) else {
        panic!("MP3 frame was not recognised");
    };
    assert_eq!(facts.format, AudioKind::Mp3);
    assert_eq!(facts.sample_rate, Some(44_100));
    assert_eq!(facts.channels, Some(2));
    assert_eq!(facts.bitrate, Some(128_000));
    assert_eq!(facts.duration, None);
}

#[test]
fn mp4_moov_inside_the_prefix_yields_track_facts() {
    let bytes = mp4_video(&[(b"mvhd", 1_000, 5_000)], 1_920, 1_080);
    let Some(FileFacts::Video(facts)) =
        file_facts(header::probe_file(None, ProbeHint::default(), &bytes))
    else {
        panic!("MP4 header was not recognised");
    };
    assert_eq!(facts.format, VideoKind::Mp4);
    assert_eq!(facts.duration, Some(Duration::from_secs(5)));
    assert_eq!(
        facts.dimensions,
        Some(PixelSize {
            width: 1_920,
            height: 1_080
        })
    );
    assert_eq!(facts.codec.as_deref(), Some("H.264"));
}

#[test]
fn mp4_without_a_moov_in_the_prefix_keeps_only_the_container_format() {
    let bytes = mp4_video(&[(b"mvhd", 1_000, 5_000)], 1_920, 1_080);
    let ftyp_len = u32::from_be_bytes(bytes[..4].try_into().expect("box size")) as usize;
    let ftyp_only = &bytes[..ftyp_len];
    let Some(FileFacts::Video(facts)) =
        file_facts(header::probe_file(None, ProbeHint::default(), ftyp_only))
    else {
        panic!("MP4 brand was not recognised");
    };
    assert_eq!(facts.format, VideoKind::Mp4);
    assert_eq!(facts.duration, None);
    assert_eq!(facts.dimensions, None);
    assert_eq!(facts.codec, None);
}

#[test]
fn utf8_bom_and_crlf_are_reported_from_the_sample() {
    let bytes = b"\xef\xbb\xbfhello\r\nworld\r\n";
    let Some(FileFacts::Text(facts)) = file_facts(header::probe_file(
        None,
        ProbeHint {
            text: true,
            ..ProbeHint::default()
        },
        bytes,
    )) else {
        panic!("UTF-8 text was not recognised");
    };
    assert_eq!(facts.encoding, TextEncoding::Utf8Bom);
    assert_eq!(facts.line_endings, Some(LineEndings::Crlf));
}

#[test]
fn mixed_line_endings_are_reported_as_mixed() {
    let bytes = b"one\ntwo\r\nthree\r";
    let Some(FileFacts::Text(facts)) = file_facts(header::probe_file(
        None,
        ProbeHint {
            text: true,
            ..ProbeHint::default()
        },
        bytes,
    )) else {
        panic!("UTF-8 text was not recognised");
    };
    assert_eq!(facts.encoding, TextEncoding::Utf8);
    assert_eq!(facts.line_endings, Some(LineEndings::Mixed));
}

#[test]
fn a_multibyte_character_cut_by_the_prefix_is_still_utf8() {
    let mut bytes = "hé".as_bytes().to_vec();
    bytes.truncate(2);
    let Some(FileFacts::Text(facts)) = file_facts(header::probe_file(
        None,
        ProbeHint {
            text: true,
            ..ProbeHint::default()
        },
        &bytes,
    )) else {
        panic!("truncated UTF-8 sample was rejected");
    };
    assert_eq!(facts.encoding, TextEncoding::Utf8);
}

#[test]
fn binary_content_is_not_presented_as_text() {
    let bytes = [0x00, 0x01, 0x02, 0x03];
    assert!(matches!(
        header::probe_file(
            None,
            ProbeHint {
                text: true,
                ..ProbeHint::default()
            },
            &bytes
        ),
        Probe::File(FileFacts::Unknown)
    ));
}

#[test]
fn prefix_read_respects_the_byte_budget() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("large.bin");
    let mut file = std::fs::File::create(&path).expect("fixture file");
    file.write_all(&vec![0u8; header::PREFIX_BYTES * 3])
        .expect("fixture bytes");
    drop(file);
    let bytes = header::read_prefix(&path, header::PREFIX_BYTES).expect("bounded read");
    assert_eq!(bytes.len(), header::PREFIX_BYTES);
}

#[test]
fn prefix_read_refuses_a_directory() {
    let directory = tempfile::tempdir().expect("temporary directory");
    assert!(header::read_prefix(directory.path(), header::PREFIX_BYTES).is_err());
}

#[test]
fn directory_probe_counts_one_level() {
    let directory = tempfile::tempdir().expect("temporary directory");
    std::fs::create_dir(directory.path().join("folder")).expect("fixture folder");
    std::fs::write(directory.path().join("note.txt"), b"hi").expect("fixture file");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        directory.path().join("note.txt"),
        directory.path().join("link"),
    )
    .expect("fixture link");

    let Probe::Directory(facts) = header::probe_directory(directory.path()) else {
        panic!("directory probe returned another variant");
    };
    assert_eq!(facts.folders, 1);
    assert_eq!(facts.files, 1);
    #[cfg(unix)]
    assert_eq!(facts.links, 1);
    assert!(!facts.capped);
}

#[test]
fn dimensions_aspect_ratio_and_media_units_are_rendered() {
    assert_eq!(
        info::dimensions(PixelSize {
            width: 1_920,
            height: 1_080
        }),
        "1920 × 1080"
    );
    assert_eq!(
        info::aspect_ratio(PixelSize {
            width: 1_920,
            height: 1_080
        }),
        "16:9"
    );
    assert_eq!(
        info::aspect_ratio(PixelSize {
            width: 1_024,
            height: 683
        }),
        "1.50:1"
    );
    assert_eq!(info::duration(Duration::from_secs(65)), "1:05");
    assert_eq!(info::duration(Duration::from_secs(3_725)), "1:02:05");
    assert_eq!(info::duration(Duration::from_millis(500)), "0.5 s");
    assert_eq!(info::sample_rate(44_100), "44.1 kHz");
    assert_eq!(info::sample_rate(48_000), "48 kHz");
    assert_eq!(info::bitrate(1_411_200), "1411 kbit/s");
    assert_eq!(info::bitrate(800), "800 bit/s");
    assert_eq!(info::count(256, true), "≥ 256");
}

fn file_facts(probe: Probe) -> Option<FileFacts> {
    match probe {
        Probe::File(facts) => Some(facts),
        _ => None,
    }
}

fn wav_header(data_size: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data_size).to_le_bytes());
    bytes.extend(b"WAVE");
    bytes.extend(b"fmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(44_100u32.to_le_bytes());
    bytes.extend(176_400u32.to_le_bytes());
    bytes.extend(4u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data_size.to_le_bytes());
    bytes
}

fn flac_header(sample_rate: u32, channels: u8, bit_depth: u8, total_samples: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(b"fLaC");
    bytes.push(0x80);
    bytes.extend([0u8, 0u8, 34]);
    bytes.extend([0u8; 10]);
    let packed = (u64::from(sample_rate & 0x000f_ffff) << 44)
        | ((u64::from(channels - 1) & 0x7) << 41)
        | ((u64::from(bit_depth - 1) & 0x1f) << 36)
        | (total_samples & 0x0000_000f_ffff_ffff);
    bytes.extend(packed.to_be_bytes());
    bytes.extend([0u8; 16]);
    bytes
}

/// Build an MP4 whose movie header and one video track sit before `mdat`.
fn mp4_video(movie_header: &[(&[u8; 4], u32, u32)], width: u16, height: u16) -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", b"isom\x00\x00\x02\x00isom");
    let mut moov_children = Vec::new();
    for &(kind, timescale, duration) in movie_header {
        let mut body = vec![0u8; 8];
        body.extend(timescale.to_be_bytes());
        body.extend(duration.to_be_bytes());
        moov_children.extend(mp4_full_box(kind, &body));
    }
    let mut tkhd_body = vec![0u8; 72];
    tkhd_body.extend((u32::from(width) << 16).to_be_bytes());
    tkhd_body.extend((u32::from(height) << 16).to_be_bytes());
    let tkhd = mp4_full_box(b"tkhd", &tkhd_body);
    let mut mdhd_body = vec![0u8; 8];
    mdhd_body.extend(90_000u32.to_be_bytes());
    mdhd_body.extend(450_000u32.to_be_bytes());
    let mdhd = mp4_full_box(b"mdhd", &mdhd_body);
    let mut hdlr_body = vec![0u8; 4];
    hdlr_body.extend(b"vide");
    hdlr_body.extend([0u8; 4]);
    let hdlr = mp4_full_box(b"hdlr", &hdlr_body);
    let mut sample_entry = vec![0u8; 24];
    sample_entry.extend(width.to_be_bytes());
    sample_entry.extend(height.to_be_bytes());
    let entry_count = 1u32.to_be_bytes();
    let sample_box = mp4_box(b"avc1", &sample_entry);
    let stsd_payload: Vec<u8> = [&[0u8; 4][..], &entry_count[..], &sample_box[..]].concat();
    let stsd = mp4_box(b"stsd", &stsd_payload);
    let minf = mp4_box(b"minf", &mp4_box(b"stbl", &stsd));
    let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
    let trak = mp4_box(b"trak", &[tkhd, mdia].concat());
    moov_children.extend(trak);
    let moov = mp4_box(b"moov", &moov_children);
    [ftyp, moov].concat()
}

fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(((8 + payload.len()) as u32).to_be_bytes());
    bytes.extend(kind);
    bytes.extend(payload);
    bytes
}

fn mp4_full_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut payload = vec![0u8; 4];
    payload.extend(body);
    mp4_box(kind, &payload)
}
