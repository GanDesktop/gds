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

#[cfg(unix)]
fn write_executable_script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;

    let path = dir.join(name);
    std::fs::write(&path, body).expect("write helper script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod +x helper script");
    path
}

#[cfg(unix)]
fn write_fake_video_source(dir: &Path, name: &str) -> Source {
    let path = dir.join(name);
    std::fs::write(&path, b"not-a-real-mp4-for-mock-thumbnailer").expect("write mock video");
    let meta = std::fs::metadata(&path).expect("stat mock video");
    Source {
        path,
        kind: Kind::Video,
        mime: Some(std::sync::Arc::from("video/mp4")),
        mime_ancestors: Vec::new(),
        size: Some(meta.len()),
        modified: meta.modified().ok(),
    }
}

#[cfg(unix)]
#[test]
fn transient_external_timeout_auto_retries_once_and_recovers() {
    use std::sync::Arc;

    use crate::service::file_inspection::{
        ExternalThumbnailers, SandboxMode, invalidate_thumbnail_cache,
    };

    install().ok();
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_test_png(dir.path(), "fixture.png", [30, 160, 210, 255], 16);
    let source = write_fake_video_source(dir.path(), "timeout_once.mp4");
    let counter = dir.path().join("attempts");
    let script = write_executable_script(
        dir.path(),
        "flaky-thumb.sh",
        &format!(
            "#!/bin/sh\ncount=0\nif [ -f \"{}\" ]; then count=$(cat \"{}\"); fi\ncount=$((count + 1))\necho \"$count\" > \"{}\"\nif [ \"$count\" -eq 1 ]; then sleep 3; fi\ncp \"{}\" \"$2\"\n",
            counter.display(),
            counter.display(),
            counter.display(),
            fixture.path.display(),
        ),
    );
    let external = ExternalThumbnailers::from_specs_for_test(
        SandboxMode::Unsandboxed,
        "flaky-timeout",
        script.clone(),
        vec![
            script.display().to_string(),
            "%i".to_owned(),
            "%o".to_owned(),
        ],
        vec!["video/mp4".to_owned()],
        false,
    );

    let scope = Mounted::new();
    scope.with(|| {
        let mut opts = Options {
            workers: 1,
            ..Options::default()
        };
        opts.inspection.external_thumbnail_timeout = Duration::from_secs(1);
        opts.inspection.external_thumbnailers = Some(Arc::new(external));
        let cache = Cache::new(opts);

        let initial = cache.snapshot(&source, Request::Thumbnail);
        assert!(matches!(initial.thumbnail, State::Loading));

        let deadline = Instant::now() + Duration::from_secs(6);
        while !matches!(
            cache.peek(&source),
            Some(ref snap) if matches!(snap.thumbnail, State::Ready(_))
        ) {
            assert!(
                Instant::now() < deadline,
                "expected transient timeout to auto-retry and recover to Ready"
            );
            flush();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(&counter)
                .expect("read attempts")
                .trim(),
            "2"
        );
    });
    scope.unmount();
    invalidate_thumbnail_cache(&source.path);
}

#[cfg(unix)]
#[test]
fn repeated_transient_timeouts_settle_failed_and_recover_on_retry_failed_in() {
    use std::sync::Arc;

    use crate::service::file_inspection::{
        ExternalThumbnailers, SandboxMode, invalidate_thumbnail_cache,
    };

    install().ok();
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_test_png(dir.path(), "fixture.png", [210, 120, 40, 255], 16);
    let source = write_fake_video_source(dir.path(), "timeout_twice.mp4");
    let counter = dir.path().join("attempts");
    let script = write_executable_script(
        dir.path(),
        "twice-timeout.sh",
        &format!(
            "#!/bin/sh\ncount=0\nif [ -f \"{}\" ]; then count=$(cat \"{}\"); fi\ncount=$((count + 1))\necho \"$count\" > \"{}\"\nif [ \"$count\" -le 2 ]; then sleep 3; fi\ncp \"{}\" \"$2\"\n",
            counter.display(),
            counter.display(),
            counter.display(),
            fixture.path.display(),
        ),
    );
    let external = ExternalThumbnailers::from_specs_for_test(
        SandboxMode::Unsandboxed,
        "twice-timeout",
        script.clone(),
        vec![
            script.display().to_string(),
            "%i".to_owned(),
            "%o".to_owned(),
        ],
        vec!["video/mp4".to_owned()],
        false,
    );

    let scope = Mounted::new();
    scope.with(|| {
        let mut opts = Options {
            workers: 1,
            ..Options::default()
        };
        opts.inspection.external_thumbnail_timeout = Duration::from_secs(1);
        opts.inspection.external_thumbnailers = Some(Arc::new(external));
        let cache = Cache::new(opts);

        let _ = cache.snapshot(&source, Request::Thumbnail);
        let deadline = Instant::now() + Duration::from_secs(8);
        while !matches!(
            cache.peek(&source),
            Some(ref snap) if matches!(snap.thumbnail, State::Failed(_))
        ) {
            assert!(
                Instant::now() < deadline,
                "expected repeated transient timeouts to settle in State::Failed"
            );
            flush();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(&counter)
                .expect("read attempts")
                .trim(),
            "2",
            "auto-retry must be bounded to 1 retry (2 total attempts)"
        );

        // Refreshing the directory clears failed entries so the next read retries cleanly.
        cache.retry_failed_in(dir.path());
        assert!(cache.peek(&source).is_none());

        let retried = cache.snapshot(&source, Request::Thumbnail);
        assert!(matches!(retried.thumbnail, State::Loading));
        drive_until(|| {
            matches!(
                cache.peek(&source),
                Some(ref snap) if matches!(snap.thumbnail, State::Ready(_))
            )
        });
        assert_eq!(
            std::fs::read_to_string(&counter)
                .expect("read attempts")
                .trim(),
            "3"
        );
    });
    scope.unmount();
    invalidate_thumbnail_cache(&source.path);
}

#[cfg(unix)]
#[test]
fn invalidate_other_path_does_not_strand_unrelated_in_flight_job() {
    use std::sync::Arc;

    use crate::service::file_inspection::{
        ExternalThumbnailers, SandboxMode, invalidate_thumbnail_cache,
    };

    install().ok();
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_test_png(dir.path(), "fixture.png", [60, 200, 90, 255], 16);
    let other = write_test_png(dir.path(), "other.png", [180, 50, 50, 255], 16);
    let source = write_fake_video_source(dir.path(), "in_flight.mp4");
    let script = write_executable_script(
        dir.path(),
        "slow-ok.sh",
        &format!(
            "#!/bin/sh\nsleep 0.15\ncp \"{}\" \"$2\"\n",
            fixture.path.display()
        ),
    );
    let external = ExternalThumbnailers::from_specs_for_test(
        SandboxMode::Unsandboxed,
        "slow-ok",
        script.clone(),
        vec![
            script.display().to_string(),
            "%i".to_owned(),
            "%o".to_owned(),
        ],
        vec!["video/mp4".to_owned()],
        false,
    );

    let scope = Mounted::new();
    scope.with(|| {
        let mut opts = Options {
            workers: 2,
            ..Options::default()
        };
        opts.inspection.external_thumbnailers = Some(Arc::new(external));
        let cache = Cache::new(opts);

        let initial = cache.snapshot(&source, Request::Thumbnail);
        assert!(matches!(initial.thumbnail, State::Loading));
        assert_eq!(cache.running_count(), 1);

        // Invalidating an unrelated path must not discard or strand the running job.
        cache.invalidate(&other.path);

        drive_until(|| {
            matches!(
                cache.peek(&source),
                Some(ref snap) if matches!(snap.thumbnail, State::Ready(_))
            )
        });
    });
    scope.unmount();
    invalidate_thumbnail_cache(&source.path);
}
