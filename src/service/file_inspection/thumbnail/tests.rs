use super::*;
use std::{fs, path::PathBuf, process::Command};

fn source(path: PathBuf, kind: Kind) -> Source {
    let metadata = fs::metadata(&path).expect("fixture metadata");
    Source {
        path,
        kind,
        size: Some(metadata.len()),
        modified: metadata.modified().ok(),
    }
}

#[test]
fn images_use_their_bytes_and_preserve_aspect_ratio_in_a_bounded_frame() {
    let directory = tempfile::tempdir().expect("image directory");
    let path = directory.path().join("misnamed.jpg");
    ::image::RgbImage::from_pixel(1200, 600, ::image::Rgb([20, 80, 140]))
        .save_with_format(&path, ::image::ImageFormat::Png)
        .expect("PNG fixture with misleading extension");
    let thumbnail = source(path, Kind::Image)
        .thumbnail()
        .expect("image preview");
    assert_eq!((thumbnail.width(), thumbnail.height()), (EDGE, EDGE / 2));
}

#[test]
fn svg_content_is_supported_without_an_svg_extension_and_large_images_are_rejected() {
    let directory = tempfile::tempdir().expect("image directory");
    let path = directory.path().join("drawing.bin");
    let svg =
        br##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#4080c0" fill-opacity="0.5"/></svg>"##;
    fs::write(&path, svg).expect("SVG fixture");
    let thumbnail = source(path.clone(), Kind::Image)
        .thumbnail()
        .expect("SVG preview");
    assert_eq!((thumbnail.width(), thumbnail.height()), (32, 32));
    let pixel = thumbnail.as_rgba().get_pixel(0, 0).0;
    for (actual, expected) in pixel.into_iter().zip([64_u8, 128, 192, 128]) {
        assert!(
            actual.abs_diff(expected) <= 1,
            "SVG alpha is stored without premultiplication"
        );
    }
    fs::File::create(&path)
        .expect("large fixture")
        .set_len(MAX_SOURCE_BYTES + 1)
        .expect("sparse fixture");
    assert!(matches!(
        source(path, Kind::Image).thumbnail(),
        Err(Error::TooLarge)
    ));
}

#[test]
fn videos_extract_one_frame_and_corrupt_media_reports_failure() {
    let directory = tempfile::tempdir().expect("video directory");
    let path = directory.path().join("视频 [one].mp4");
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=64x48:d=0.12",
            "-c:v",
            "mpeg4",
            "-threads",
            "1",
            "-y",
        ])
        .arg(&path)
        .output()
        .expect("ffmpeg is part of the development and test environment");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let thumbnail = source(path.clone(), Kind::Video)
        .thumbnail()
        .expect("short video preview");
    assert!(thumbnail.width() <= EDGE && thumbnail.height() <= EDGE);
    assert_eq!(thumbnail.width() * 3, thumbnail.height() * 4);
    fs::write(&path, b"not a video").expect("corrupt video");
    assert!(matches!(
        source(path, Kind::Video).thumbnail(),
        Err(Error::Video(_))
    ));
}
