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
            assert_eq!(
                &first.as_bytes(0).expect("decoded pixels")[..4],
                &[30, 20, 10, 255]
            );
            assert_eq!(
                cache.bytes,
                first.as_bytes(0).expect("decoded pixels").len()
            );
            assert!(cache.bytes > std::fs::metadata(&path).expect("source size").len() as usize);
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

#[gpui_kit::test]
fn paging_keeps_previous_previews_ready(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("thumbnail directory");
    let pages: Vec<Vec<Source>> = (0..3)
        .map(|page| {
            (0..16)
                .map(|index| {
                    let path = directory.path().join(format!("{page}-{index}.png"));
                    image::RgbImage::from_pixel(8, 4, image::Rgb([page, index, 30]))
                        .save(&path)
                        .expect("image fixture");
                    Source {
                        path,
                        kind: Kind::Image,
                        size: Some(1),
                        modified: None,
                    }
                })
                .collect()
        })
        .collect();
    let cache = cx.update(|cx| cx.new(|_| Cache::default()));
    let mut images = Vec::new();
    for page in &pages {
        cx.update(|cx| {
            cache.update(cx, |cache, cx| {
                for source in page {
                    cache.request(source.clone(), cx);
                }
            })
        });
        cx.run_until_parked();
        images.push(cx.update(|cx| {
            cache.update(cx, |cache, cx| {
                page.iter()
                    .map(|source| {
                        let State::Ready(image) = cache.request(source.clone(), cx) else {
                            panic!("page must finish loading");
                        };
                        image
                    })
                    .collect::<Vec<_>>()
            })
        }));
    }
    for (page, images) in pages.iter().zip(&images).rev() {
        cx.update(|cx| {
            cache.update(cx, |cache, cx| {
                for (source, original) in page.iter().zip(images) {
                    let State::Ready(current) = cache.request(source.clone(), cx) else {
                        panic!("returning to a page must be immediately ready");
                    };
                    assert!(Arc::ptr_eq(original, &current));
                }
                assert!(cache.pending.is_empty());
                assert_eq!(cache.running, 0);
            })
        });
    }
}
