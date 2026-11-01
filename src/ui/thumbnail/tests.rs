use super::{Cache, Kind, Source, State};
use gpui_kit::{AppContext, TestAppContext};
use std::sync::Arc;

#[gpui_kit::test]
fn cached_images_are_shared_and_replacing_a_file_requests_a_new_preview(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("thumbnail directory");
    let path = directory.path().join("photo.png");
    image::RgbImage::from_pixel(8, 4, image::Rgb([10, 20, 30]))
        .save(&path)
        .expect("image fixture");
    let source = Source {
        path: path.clone(),
        kind: Kind::Image,
        size: Some(1),
        modified: None,
    };
    let cache = cx.update(|cx| cx.new(|_| Cache::default()));
    cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            cache.request(source.clone(), cx);
            cache.request(source.clone(), cx);
        })
    });
    cx.run_until_parked();
    let original = cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            let State::Ready(first) = cache.request(source.clone(), cx) else {
                panic!("thumbnail must finish");
            };
            let State::Ready(second) = cache.request(source.clone(), cx) else {
                panic!("cached thumbnail must stay ready");
            };
            assert!(Arc::ptr_eq(&first, &second));
            first
        })
    });
    image::RgbImage::from_pixel(16, 8, image::Rgb([120, 80, 40]))
        .save(&path)
        .expect("replacement image");
    let replacement = Source {
        size: Some(2),
        ..source
    };
    cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            assert!(matches!(
                cache.request(replacement.clone(), cx),
                State::Loading
            ))
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            let State::Ready(current) = cache.request(replacement, cx) else {
                panic!("replacement preview must finish");
            };
            assert!(!Arc::ptr_eq(&original, &current));
        })
    });
}
