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
