use std::path::Path;
use std::time::{Duration, Instant};

use image::{ImageBuffer, Rgba};
use tempfile::TempDir;
use zgui::reactive::{Mounted, flush, install};

use super::{Cache, Options, State};
use crate::service::file_inspection::{Kind, Request, Source};

fn write_test_png(dir: &Path, name: &str, color: [u8; 4], size: u32) -> Source {
    let path = dir.join(name);
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(size, size, |_, _| Rgba(color));
    img.save(&path).expect("write PNG fixture");
    let len = std::fs::metadata(&path).expect("stat PNG fixture").len();
    Source {
        path,
        kind: Kind::Image,
        mime: None,
        mime_ancestors: Vec::new(),
        size: Some(len),
        modified: None,
    }
}

fn drive_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "async inspection worker timed out"
        );
        flush();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn async_worker_decodes_png_thumbnail_and_metadata_for_real_image() {
    install().ok();
    let dir = TempDir::new().expect("tempdir");
    let source = write_test_png(dir.path(), "sample.png", [24, 144, 220, 255], 32);

    let scope = Mounted::new();
    scope.with(|| {
        let cache = Cache::new(Options::default());
        let initial = cache.snapshot(&source, Request::Both);
        assert!(matches!(initial.thumbnail, State::Loading));
        assert!(matches!(initial.metadata, State::Loading));

        drive_until(|| {
            matches!(
                cache.peek(&source),
                Some(ref snap)
                    if matches!(snap.thumbnail, State::Ready(_))
                        && matches!(snap.metadata, State::Ready(_))
            )
        });

        let ready = cache.peek(&source).expect("cached snapshot");
        match ready.thumbnail {
            State::Ready(bytes) => assert!(bytes.url().starts_with("zgui-bytes:")),
            _ => panic!("expected Ready thumbnail ImageBytes"),
        }
        assert!(cache.byte_usage() > 0);
        assert_eq!(cache.running_count(), 0);
        assert_eq!(cache.pending_count(), 0);
    });
    scope.unmount();
}

#[test]
fn lru_eviction_and_byte_budget_drop_oldest_entries() {
    install().ok();
    let dir = TempDir::new().expect("tempdir");
    let src_a = write_test_png(dir.path(), "a.png", [200, 10, 10, 255], 32);
    let src_b = write_test_png(dir.path(), "b.png", [10, 200, 10, 255], 32);
    let src_c = write_test_png(dir.path(), "c.png", [10, 10, 200, 255], 32);

    let scope = Mounted::new();
    scope.with(|| {
        let opts = Options {
            capacity: 2,
            workers: 1,
            ..Options::default()
        };
        let cache = Cache::new(opts);

        for src in [&src_a, &src_b, &src_c] {
            let _ = cache.snapshot(src, Request::Thumbnail);
            drive_until(|| {
                matches!(
                    cache.peek(src),
                    Some(ref snap) if matches!(snap.thumbnail, State::Ready(_))
                )
            });
        }

        assert_eq!(cache.entry_count(), 2);
        assert!(
            cache.peek(&src_a).is_none(),
            "oldest entry should be evicted"
        );
        assert!(cache.peek(&src_b).is_some());
        assert!(cache.peek(&src_c).is_some());

        let total_for_two = cache.byte_usage();
        cache.set_options(Options {
            capacity: 16,
            byte_budget: total_for_two.saturating_sub(1),
            workers: 1,
            ..Options::default()
        });
        assert_eq!(cache.entry_count(), 1);
        assert!(cache.peek(&src_b).is_none());
        assert!(cache.peek(&src_c).is_some());
    });
    scope.unmount();
}

#[test]
fn scoped_cache_runs_through_task_manager_supervisor() {
    install().ok();
    let supervisor =
        crate::service::task_manager::Supervisor::new(false).expect("start supervisor");
    let dir = TempDir::new().expect("tempdir");
    let source = write_test_png(dir.path(), "scoped.png", [40, 180, 120, 255], 32);

    let scope = Mounted::new();
    scope.with(|| {
        let cache = Cache::with_scope(supervisor.scope(), Options::default());
        let initial = cache.snapshot(&source, Request::Thumbnail);
        assert!(matches!(initial.thumbnail, State::Loading));

        drive_until(|| {
            matches!(
                cache.peek(&source),
                Some(ref snap) if matches!(snap.thumbnail, State::Ready(_))
            )
        });
    });
    scope.unmount();
    supervisor.shutdown();
}

#[test]
fn oversized_source_reports_too_large_state_and_clear_resets() {
    install().ok();
    let dir = TempDir::new().expect("tempdir");
    let source = write_test_png(dir.path(), "large.png", [90, 90, 90, 255], 32);

    let scope = Mounted::new();
    scope.with(|| {
        let mut opts = Options::default();
        opts.inspection.max_thumbnail_source_bytes = 8;
        let cache = Cache::new(opts);
        let _ = cache.snapshot(&source, Request::Thumbnail);

        drive_until(|| {
            matches!(
                cache.peek(&source),
                Some(ref snap) if matches!(snap.thumbnail, State::TooLarge)
            )
        });

        cache.clear();
        assert_eq!(cache.entry_count(), 0);
        assert_eq!(cache.byte_usage(), 0);
    });
    scope.unmount();
}

#[test]
fn concurrent_request_upgrade_completes_both_metadata_and_thumbnail() {
    install().ok();
    let dir = TempDir::new().expect("tempdir");
    let source = write_test_png(dir.path(), "upgrade.png", [64, 128, 255, 255], 32);

    let scope = Mounted::new();
    scope.with(|| {
        let cache = Cache::new(Options {
            workers: 1,
            ..Options::default()
        });
        let first = cache.request(&source, Request::Metadata);
        assert!(matches!(first.metadata, State::Loading));
        assert!(matches!(first.thumbnail, State::NotRequested));

        let second = cache.request(&source, Request::Thumbnail);
        assert!(matches!(second.thumbnail, State::Loading));

        drive_until(|| {
            matches!(
                cache.peek(&source),
                Some(ref snap)
                    if matches!(snap.metadata, State::Ready(_))
                        && matches!(snap.thumbnail, State::Ready(_))
            )
        });
    });
    scope.unmount();
}
