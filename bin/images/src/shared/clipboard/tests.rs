use std::path::PathBuf;

use gds::service::{
    image::source::{Format, MemoryOrigin, Origin},
    task_manager::Supervisor,
};
use zgui::platform::{ClipboardMime, ClipboardMimeOffer};

use super::{
    ClipboardOfferKind, GNOME_FILES, URI_LIST, extract_candidate_local_image_paths,
    first_raw_mime_format, local_file_uri, offer_from_mime_offer, origin_from_mime_offer,
    parse_gnome_files_uris,
};

#[test]
fn local_file_uri_rejects_credentials_query_fragment_and_relative_uris() {
    assert_eq!(
        local_file_uri("file:///tmp/photo.png"),
        Some(PathBuf::from("/tmp/photo.png"))
    );
    assert_eq!(
        local_file_uri("file://localhost/tmp/photo%201.jpg"),
        Some(PathBuf::from("/tmp/photo 1.jpg"))
    );

    assert!(local_file_uri("file:photo.png").is_none());
    assert!(local_file_uri("file://remote-host/tmp/photo.png").is_none());
    assert!(local_file_uri("file://user@localhost/tmp/photo.png").is_none());
    assert!(local_file_uri("file://user:pass@localhost/tmp/photo.png").is_none());
    assert!(local_file_uri("file://localhost:8080/tmp/photo.png").is_none());
    assert!(local_file_uri("file:///tmp/photo.png?raw=1").is_none());
    assert!(local_file_uri("file:///tmp/photo.png#section").is_none());
    assert!(local_file_uri("file:///tmp/photo\0.png").is_none());
    assert!(local_file_uri("https://example.com/photo.png").is_none());
}

#[test]
fn gnome_copied_files_requires_copy_or_cut_intent_header() {
    assert_eq!(
        parse_gnome_files_uris(b"copy\nfile:///tmp/a.png\n"),
        Some("file:///tmp/a.png\n")
    );
    assert_eq!(
        parse_gnome_files_uris(b"cut\r\nfile:///tmp/a.png"),
        Some("file:///tmp/a.png")
    );
    assert!(parse_gnome_files_uris(b"move\nfile:///tmp/a.png").is_none());
    assert!(parse_gnome_files_uris(b"file:///tmp/a.png").is_none());
}

#[test]
fn offer_extraction_filters_supported_formats_and_detects_actual_mime() {
    let offer = ClipboardMimeOffer {
        revision: 1,
        offered_mime_types: vec![GNOME_FILES.into(), URI_LIST.into()],
        entries: vec![
            ClipboardMime {
                mime_type: GNOME_FILES.into(),
                bytes: b"copy\nfile:///tmp/notes.txt\nfile:///tmp/pic.webp?bad=1\nfile:///tmp/pic.webp\n".to_vec(),
            },
            ClipboardMime {
                mime_type: URI_LIST.into(),
                bytes: b"file:///tmp/ignored.png".to_vec(),
            },
        ],
    };
    assert_eq!(
        extract_candidate_local_image_paths(&offer),
        vec![PathBuf::from("/tmp/pic.webp")]
    );

    let invalid_gnome = ClipboardMimeOffer {
        revision: 2,
        offered_mime_types: vec![GNOME_FILES.into(), URI_LIST.into()],
        entries: vec![
            ClipboardMime {
                mime_type: GNOME_FILES.into(),
                bytes: b"bogus\nfile:///tmp/a.png".to_vec(),
            },
            ClipboardMime {
                mime_type: URI_LIST.into(),
                bytes: b"file:///tmp/a.png".to_vec(),
            },
        ],
    };
    assert!(extract_candidate_local_image_paths(&invalid_gnome).is_empty());

    let raw_webp = ClipboardMimeOffer {
        revision: 3,
        offered_mime_types: vec!["image/png".into(), "image/webp".into()],
        entries: vec![
            ClipboardMime {
                mime_type: "image/png".into(),
                bytes: Vec::new(),
            },
            ClipboardMime {
                mime_type: "image/webp".into(),
                bytes: vec![1, 2, 3, 4],
            },
        ],
    };
    assert_eq!(first_raw_mime_format(&raw_webp), Some(Format::WebP));

    let prioritized = ClipboardMimeOffer {
        revision: 4,
        offered_mime_types: vec!["image/webp".into(), "image/png".into()],
        entries: vec![
            ClipboardMime {
                mime_type: "image/webp".into(),
                bytes: vec![9, 8, 7],
            },
            ClipboardMime {
                mime_type: "image/png".into(),
                bytes: vec![1, 2, 3],
            },
        ],
    };
    assert_eq!(first_raw_mime_format(&prioritized), Some(Format::Png));
}

#[test]
fn offer_and_origin_resolution_prefers_existing_local_file_and_falls_back_to_raw_mime() {
    let supervisor = Supervisor::new(false).expect("start test supervisor");
    let tasks = supervisor.scope();
    let dir = tempfile::tempdir().expect("temp dir");
    let image_path = dir.path().join("sample.png");
    std::fs::write(&image_path, b"png-bytes").expect("write sample image");
    let file_uri = url::Url::from_file_path(&image_path)
        .expect("file uri")
        .to_string();

    let with_local = ClipboardMimeOffer {
        revision: 10,
        offered_mime_types: vec![URI_LIST.into(), "image/webp".into()],
        entries: vec![
            ClipboardMime {
                mime_type: URI_LIST.into(),
                bytes: format!("{file_uri}\n").into_bytes(),
            },
            ClipboardMime {
                mime_type: "image/webp".into(),
                bytes: vec![4, 5, 6],
            },
        ],
    };
    let probed_local = supervisor
        .block_on(offer_from_mime_offer(with_local.clone(), &tasks))
        .expect("block on local probe");
    assert_eq!(
        probed_local,
        Some(ClipboardOfferKind::LocalImageFile(image_path.clone()))
    );
    let resolved_local = supervisor
        .block_on(origin_from_mime_offer(with_local, &tasks))
        .expect("block on local origin");
    assert!(matches!(
        resolved_local,
        Some(Origin::LocalFile { path, .. }) if path == image_path
    ));

    let missing_uri = url::Url::from_file_path(dir.path().join("missing.png"))
        .expect("missing uri")
        .to_string();
    let fallback_raw = ClipboardMimeOffer {
        revision: 11,
        offered_mime_types: vec![URI_LIST.into(), "image/webp".into()],
        entries: vec![
            ClipboardMime {
                mime_type: URI_LIST.into(),
                bytes: format!("{missing_uri}\n").into_bytes(),
            },
            ClipboardMime {
                mime_type: "image/webp".into(),
                bytes: vec![7, 8, 9],
            },
        ],
    };
    let probed_raw = supervisor
        .block_on(offer_from_mime_offer(fallback_raw.clone(), &tasks))
        .expect("block on raw probe");
    assert_eq!(
        probed_raw,
        Some(ClipboardOfferKind::RawImageMime(Format::WebP))
    );
    let resolved_raw = supervisor
        .block_on(origin_from_mime_offer(fallback_raw, &tasks))
        .expect("block on raw origin");
    assert!(matches!(
        resolved_raw,
        Some(Origin::Memory {
            kind: MemoryOrigin::Clipboard,
            bytes,
            format_hint: Some(Format::WebP),
        }) if bytes.as_ref() == [7, 8, 9]
    ));

    let empty_offer = ClipboardMimeOffer {
        revision: 12,
        offered_mime_types: vec!["image/png".into()],
        entries: vec![ClipboardMime {
            mime_type: "image/png".into(),
            bytes: Vec::new(),
        }],
    };
    assert_eq!(
        supervisor
            .block_on(offer_from_mime_offer(empty_offer.clone(), &tasks))
            .expect("block on empty probe"),
        None
    );
    assert_eq!(
        supervisor
            .block_on(origin_from_mime_offer(empty_offer, &tasks))
            .expect("block on empty origin"),
        None
    );

    supervisor.shutdown();
}
