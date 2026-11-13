use super::{Cache, Request, Source, State};
use crate::service::file_inspection::Kind;
use gds::app::AppExt as _;
use gpui_kit::{AppContext, TestAppContext};
use std::sync::Arc;

#[gpui_kit::test]
fn cached_images_are_shared_and_replacing_a_file_requests_a_new_preview(cx: &mut TestAppContext) {
    cx.update(|cx| cx.gds().tasks().init_test(false));
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
            cache
                .request(source.clone(), Request::Thumbnail, cx)
                .thumbnail;
            cache
                .request(source.clone(), Request::Thumbnail, cx)
                .thumbnail;
        })
    });
    cx.run_until_parked();
    let original = cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            let State::Ready(first) = cache
                .request(source.clone(), Request::Thumbnail, cx)
                .thumbnail
            else {
                panic!("thumbnail must finish");
            };
            let State::Ready(second) = cache
                .request(source.clone(), Request::Thumbnail, cx)
                .thumbnail
            else {
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
                cache
                    .request(replacement.clone(), Request::Thumbnail, cx)
                    .thumbnail,
                State::Loading
            ))
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            let State::Ready(current) =
                cache.request(replacement, Request::Thumbnail, cx).thumbnail
            else {
                panic!("replacement preview must finish");
            };
            assert!(!Arc::ptr_eq(&original, &current));
        })
    });
}

#[gpui_kit::test]
fn paging_keeps_previous_previews_ready(cx: &mut TestAppContext) {
    cx.update(|cx| cx.gds().tasks().init_test(false));
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
                    cache
                        .request(source.clone(), Request::Thumbnail, cx)
                        .thumbnail;
                }
            })
        });
        cx.run_until_parked();
        images.push(cx.update(|cx| {
            cache.update(cx, |cache, cx| {
                page.iter()
                    .map(|source| {
                        let State::Ready(image) = cache
                            .request(source.clone(), Request::Thumbnail, cx)
                            .thumbnail
                        else {
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
                    let State::Ready(current) = cache
                        .request(source.clone(), Request::Thumbnail, cx)
                        .thumbnail
                    else {
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

#[gpui_kit::test]
fn metadata_requested_during_thumbnail_loading_is_completed_and_reused(cx: &mut TestAppContext) {
    use crate::service::file_inspection::{FileFacts, Metadata};
    cx.update(|cx| cx.gds().tasks().init_test(false));
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("inspection directory");
    let path = directory.path().join("photo.png");
    image::RgbImage::new(20, 10)
        .save(&path)
        .expect("image fixture");
    let source = Source {
        path,
        kind: Kind::Image,
        size: None,
        modified: None,
    };
    let cache = cx.update(|cx| cx.new(|_| Cache::default()));
    cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            let first = cache.request(source.clone(), Request::Thumbnail, cx);
            assert!(matches!(first.metadata, State::NotRequested));
            assert!(matches!(first.thumbnail, State::Loading));
            let both = cache.request(source.clone(), Request::Both, cx);
            assert!(matches!(both.metadata, State::Loading));
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            let both = cache.request(source.clone(), Request::Both, cx);
            let State::Ready(metadata) = both.metadata else {
                panic!("metadata must complete");
            };
            let Metadata::File(FileFacts::Image(facts)) = metadata.as_ref() else {
                panic!("image metadata");
            };
            assert_eq!(facts.size.expect("dimensions").width, 20);
            assert!(matches!(both.thumbnail, State::Ready(_)));
            let State::Ready(reused) = cache.request(source, Request::Metadata, cx).metadata else {
                panic!("cached metadata");
            };
            assert!(Arc::ptr_eq(&metadata, &reused));
            assert_eq!(cache.running, 0);
            assert!(cache.pending.is_empty());
        })
    });
}

#[gpui_kit::test]
fn thumbnail_failure_keeps_readable_metadata(cx: &mut TestAppContext) {
    cx.update(|cx| cx.gds().tasks().init_test(false));
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("inspection directory");
    let path = directory.path().join("truncated.png");
    image::RgbImage::new(20, 10)
        .save(&path)
        .expect("image fixture");
    let bytes = std::fs::read(&path).expect("PNG bytes");
    std::fs::write(&path, &bytes[..33]).expect("keep signature and IHDR, truncate pixels");
    let source = Source {
        path,
        kind: Kind::Image,
        size: None,
        modified: None,
    };
    let cache = cx.update(|cx| cx.new(|_| Cache::default()));
    cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            cache.request(source.clone(), Request::Both, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            let snapshot = cache.request(source, Request::Both, cx);
            assert!(matches!(snapshot.metadata, State::Ready(_)));
            assert!(matches!(snapshot.thumbnail, State::Failed(_)));
        })
    });
}

#[test]
fn pending_queue_pops_most_recently_prioritized_item_first() {
    let mut cache = Cache::default();
    let make = |name: &str| Source {
        path: std::path::PathBuf::from(name),
        kind: Kind::Image,
        size: Some(1),
        modified: None,
    };
    let a = make("a.png");
    let b = make("b.png");
    let c = make("c.png");
    cache.enqueue_pending(a.clone());
    cache.enqueue_pending(b.clone());
    cache.enqueue_pending(c.clone());
    // Re-prioritizing `a` moves it to the back without inserting a duplicate.
    cache.enqueue_pending(a.clone());
    assert_eq!(cache.next_pending(), Some(a));
    assert_eq!(cache.next_pending(), Some(c));
    assert_eq!(cache.next_pending(), Some(b));
    assert_eq!(cache.next_pending(), None);
}

#[gpui_kit::test]
fn re_requested_pending_items_move_to_the_front_of_lifo_priority(cx: &mut TestAppContext) {
    cx.update(|cx| cx.gds().tasks().init_test(false));
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("priority directory");
    let sources: Vec<Source> = (0..5)
        .map(|index| {
            let path = directory.path().join(format!("{index}.png"));
            image::RgbImage::from_pixel(4, 4, image::Rgb([index, 20, 30]))
                .save(&path)
                .expect("image fixture");
            Source {
                path,
                kind: Kind::Image,
                size: Some(1),
                modified: None,
            }
        })
        .collect();
    let cache = cx.update(|cx| cx.new(|_| Cache::default()));
    cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            for source in &sources {
                cache.request(source.clone(), Request::Thumbnail, cx);
            }
            assert_eq!(cache.running, super::WORKERS);
            assert_eq!(
                cache.pending,
                std::collections::VecDeque::from(sources[super::WORKERS..].to_vec())
            );
            // Re-requesting an already-pending item promotes it to the back (next popped).
            cache.request(sources[super::WORKERS].clone(), Request::Thumbnail, cx);
            let mut expected = sources[(super::WORKERS + 1)..].to_vec();
            expected.push(sources[super::WORKERS].clone());
            assert_eq!(cache.pending, std::collections::VecDeque::from(expected));
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        cache.update(cx, |cache, cx| {
            for source in &sources {
                assert!(matches!(
                    cache
                        .request(source.clone(), Request::Thumbnail, cx)
                        .thumbnail,
                    State::Ready(_)
                ));
            }
            assert!(cache.pending.is_empty());
            assert_eq!(cache.running, 0);
        })
    });
}
