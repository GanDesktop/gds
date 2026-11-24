use super::*;
use std::{fs, path::PathBuf, sync::Arc};

fn source(path: PathBuf, kind: Kind) -> Source {
    let metadata = fs::metadata(&path).expect("fixture metadata");
    Source {
        path,
        kind,
        mime: None,
        mime_ancestors: Vec::new(),
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

#[cfg(unix)]
#[test]
fn videos_delegate_to_external_thumbnailers_when_configured() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().expect("video directory");
    let path = directory.path().join("视频 [one].mp4");
    fs::write(&path, b"not a real video").expect("video fixture");
    let mut video_source = source(path, Kind::Video);
    video_source.mime = Some(Arc::from("video/mp4"));

    // Without an external thumbnailer, videos are not handled built-in.
    assert!(!video_source.supports_thumbnail(&super::super::Options::default()));

    let fixture_png = directory.path().join("frame.png");
    ::image::RgbaImage::from_pixel(64, 48, ::image::Rgba([0, 0, 255, 255]))
        .save(&fixture_png)
        .expect("fixture png");

    let script = directory.path().join("mock-ffmpegthumbnailer");
    fs::write(
        &script,
        format!("#!/bin/sh\ncp '{}' \"$2\"\n", fixture_png.display()),
    )
    .expect("write helper");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod +x");

    let runner = ExternalThumbnailers::from_specs_for_test(
        SandboxMode::Unsandboxed,
        "ffmpegthumbnailer",
        script.clone(),
        vec![script.display().to_string(), "%i".into(), "%o".into()],
        vec!["video/mp4".into()],
        false,
    );
    let options = super::super::Options {
        external_thumbnailers: Some(Arc::new(runner)),
        ..super::super::Options::default()
    };
    assert!(video_source.supports_thumbnail(&options));

    let generated = video_source
        .generate_cancellable_with_options(&options, &tokio_util::sync::CancellationToken::new())
        .expect("external video thumbnail");
    assert_eq!(
        (generated.image.width(), generated.image.height()),
        (64, 48)
    );

    // A helper exiting non-zero reports ExternalFailed.
    let failing = directory.path().join("failing-thumb");
    fs::write(&failing, "#!/bin/sh\necho bad stream >&2\nexit 1\n").expect("write failing");
    fs::set_permissions(&failing, fs::Permissions::from_mode(0o755)).expect("chmod +x");
    let failing_runner = ExternalThumbnailers::from_specs_for_test(
        SandboxMode::Unsandboxed,
        "ffmpegthumbnailer",
        failing.clone(),
        vec![failing.display().to_string(), "%i".into(), "%o".into()],
        vec!["video/mp4".into()],
        false,
    );
    let failing_options = super::super::Options {
        external_thumbnailers: Some(Arc::new(failing_runner)),
        ..super::super::Options::default()
    };
    assert!(matches!(
        video_source.generate_cancellable_with_options(
            &failing_options,
            &tokio_util::sync::CancellationToken::new()
        ),
        Err(Error::ExternalFailed(_))
    ));
}
