use super::*;
use crate::service::file_inspection::{Kind, Source};
use image::{Rgb, RgbImage, Rgba};
use std::time::Duration;

struct Fixture {
    _directory: tempfile::TempDir,
    cache: Cache,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("thumbnail cache fixture");
        Self {
            cache: Cache {
                root: directory.path().join("thumbnails"),
            },
            path: directory.path().join("source.bmp"),
            _directory: directory,
        }
    }

    fn image(&self, color: [u8; 3], modified: SystemTime) {
        RgbImage::from_pixel(1200, 600, Rgb(color))
            .save(&self.path)
            .expect("source image");
        File::open(&self.path)
            .expect("source file")
            .set_times(fs::FileTimes::new().set_modified(modified))
            .expect("source timestamp");
    }

    fn identity(&self) -> Identity {
        Identity::new(
            &self.path,
            &fs::metadata(&self.path).expect("source metadata"),
        )
        .expect("source identity")
    }

    fn source(&self) -> Source {
        Source {
            path: self.path.clone(),
            kind: Kind::Image,
            mime: None,
            mime_ancestors: Vec::new(),
            size: None,
            modified: None,
        }
    }

    fn load(&self) -> Result<Thumbnail, Error> {
        let source = self.source();
        load_with(source.thumbnail_target(), Some(&self.cache), || {
            source.generate()
        })
    }

    fn cached(&self) -> PathBuf {
        self.cache
            .root
            .join(WRITE_FLAVOR)
            .join(self.identity().filename())
    }
}

fn color(thumbnail: &Thumbnail) -> [u8; 4] {
    thumbnail.as_rgba().get_pixel(0, 0).0
}

// Produced independently of our encoder: interoperable PNG text chunks may be
// compressed or UTF-8, and optional original-size/nanosecond fields may be absent.
fn external_png(identity: &Identity) -> Vec<u8> {
    let image = RgbaImage::from_pixel(512, 256, Rgba([0, 0, 200, 255]));
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 512, 256);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .add_itxt_chunk("Thumb::URI".into(), identity.uri.clone())
        .expect("external URI");
    encoder
        .add_ztxt_chunk("Thumb::MTime".into(), identity.mtime.to_string())
        .expect("external timestamp");
    let mut writer = encoder.write_header().expect("external header");
    writer
        .write_image_data(image.as_raw())
        .expect("external pixels");
    writer.finish().expect("external PNG end");
    bytes
}

#[test]
fn filename_matches_the_freedesktop_reference_and_raw_paths_are_uri_encoded() {
    let uri = file_uri(Path::new("/home/jens/photos/me.png")).expect("reference URI");
    assert_eq!(uri, "file:///home/jens/photos/me.png");
    assert_eq!(
        format!("{:x}.png", Md5::digest(uri)),
        "c6ee772d9e49320e97ec29a7eb5b1697.png"
    );
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let path = PathBuf::from(OsString::from_vec(b"/tmp/raw #\xff.png".to_vec()));
        assert_eq!(
            file_uri(&path).expect("raw file URI"),
            "file:///tmp/raw%20%23%FF.png"
        );
    }
}

#[test]
fn generated_pngs_have_standard_metadata_permissions_and_reuse_other_thumbnailers() {
    let fixture = Fixture::new();
    fixture.image([200, 0, 0], UNIX_EPOCH + Duration::new(10, 100));
    let thumbnail = fixture.load().expect("generated thumbnail");
    assert_eq!((thumbnail.width(), thumbnail.height()), (512, 256));
    let cached = read_png(&fixture.cached(), EDGE)
        .expect("cache file")
        .expect("saved PNG");
    assert_eq!(&cached.image, thumbnail.as_rgba());
    let identity = fixture.identity();
    assert_eq!(cached.attributes.get("Thumb::URI"), Some(&identity.uri));
    assert_eq!(
        cached.attributes.get("Thumb::MTime").map(String::as_str),
        Some("10")
    );
    assert_eq!(
        cached.attributes.get("Thumb::Size"),
        Some(
            &fs::metadata(&fixture.path)
                .expect("metadata")
                .len()
                .to_string()
        )
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&fixture.cache.root, &fixture.cache.root.join(WRITE_FLAVOR)] {
            assert_eq!(
                fs::metadata(path)
                    .expect("cache directory")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        assert_eq!(
            fs::metadata(fixture.cached())
                .expect("cache PNG")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    fs::write(fixture.cached(), external_png(&identity)).expect("another application's cached PNG");
    assert_eq!(
        color(&fixture.load().expect("reused thumbnail")),
        [0, 0, 200, 255]
    );
    let larger = fixture.cache.root.join("xx-large");
    private_directory(&larger).expect("larger cache class");
    let larger = larger.join(identity.filename());
    fs::rename(fixture.cached(), &larger).expect("existing larger thumbnail");
    assert_eq!(
        color(&fixture.load().expect("reuse larger size class")),
        [0, 0, 200, 255]
    );
    fs::remove_file(larger).expect("remove external thumbnail");
    fs::write(fixture.cached(), b"corrupt PNG").expect("corrupt shared cache");
    assert_eq!(
        color(&fixture.load().expect("regenerated thumbnail")),
        [200, 0, 0, 255]
    );
}

#[test]
fn equally_sized_quick_edits_and_older_timestamps_invalidate_cached_previews() {
    let fixture = Fixture::new();
    fixture.image([200, 0, 0], UNIX_EPOCH + Duration::new(10, 100));
    fixture.load().expect("original preview");
    let before = fixture.identity();
    fixture.image([0, 200, 0], UNIX_EPOCH + Duration::new(10, 200));
    assert_eq!(fixture.identity().size, before.size);
    assert_eq!(fixture.identity().mtime, before.mtime);
    assert_eq!(
        color(&fixture.load().expect("subsecond edit")),
        [0, 200, 0, 255]
    );
    fixture.image([0, 0, 200], UNIX_EPOCH + Duration::from_secs(9));
    assert_eq!(
        color(&fixture.load().expect("older replacement")),
        [0, 0, 200, 255]
    );
}

#[test]
fn cache_identity_checks_uri_and_optional_size_before_serving_pixels() {
    let fixture = Fixture::new();
    fixture.image([200, 0, 0], UNIX_EPOCH + Duration::from_secs(10));
    let mut other_identity = fixture.identity();
    other_identity.uri.push_str("-another-file");
    fixture
        .cache
        .write(
            &fixture.cache.root.join(WRITE_FLAVOR),
            &fixture.identity().filename(),
            &external_png(&other_identity),
        )
        .expect("misidentified cache");
    assert_eq!(
        color(&fixture.load().expect("correct URI")),
        [200, 0, 0, 255]
    );
    let mut wrong_size = fixture.identity();
    wrong_size.size += 1;
    let bytes = encode(
        &RgbaImage::from_pixel(2, 2, Rgba([0, 0, 200, 255])),
        Some(&wrong_size),
        ExtraMeta::default(),
        None,
    )
    .expect("wrong size PNG");
    fs::write(fixture.cached(), bytes).expect("cache for a previous file size");
    assert_eq!(
        color(&fixture.load().expect("correct source size")),
        [200, 0, 0, 255]
    );
}

#[test]
fn failure_markers_are_per_application_and_replaced_after_the_source_is_fixed() {
    let fixture = Fixture::new();
    fs::write(&fixture.path, b"broken bitmap").expect("corrupt source");
    assert!(matches!(fixture.load(), Err(Error::Image(_))));
    let failure = fixture.cache.failure_path(&fixture.identity());
    assert!(failure.is_file());
    assert!(matches!(fixture.load(), Err(Error::CachedFailure)));
    fixture
        .cache
        .write(
            &fixture.cache.root.join(WRITE_FLAVOR),
            &fixture.identity().filename(),
            &external_png(&fixture.identity()),
        )
        .expect("another decoder succeeds");
    assert_eq!(
        color(
            &fixture
                .load()
                .expect("successful cache wins over our fail marker")
        ),
        [0, 0, 200, 255]
    );
    fixture.image([200, 0, 0], UNIX_EPOCH + Duration::from_secs(10));
    assert_eq!(
        color(&fixture.load().expect("repaired source")),
        [200, 0, 0, 255]
    );
    assert!(!failure.exists());
}

#[test]
fn cache_errors_do_not_prevent_previews_and_missing_originals_are_not_served() {
    let fixture = Fixture::new();
    fixture.image([200, 0, 0], UNIX_EPOCH + Duration::from_secs(10));
    fs::write(&fixture.cache.root, b"not a cache directory").expect("unusable cache");
    assert_eq!(
        color(&fixture.load().expect("preview without disk cache")),
        [200, 0, 0, 255]
    );
    fs::remove_file(&fixture.cache.root).expect("remove obstruction");
    fixture.load().expect("cached preview");
    fs::remove_file(&fixture.path).expect("source removed");
    assert!(
        matches!(fixture.load(), Err(Error::Io(error)) if error.kind() == io::ErrorKind::NotFound)
    );
}

#[test]
fn cache_files_are_not_thumbnailed_into_the_cache_again() {
    let mut fixture = Fixture::new();
    let directory = fixture.cache.root.join("normal");
    fs::create_dir_all(&directory).expect("existing thumbnail directory");
    fixture.path = directory.join("cached.png");
    fixture.image([200, 0, 0], UNIX_EPOCH + Duration::from_secs(10));
    fixture.load().expect("display a cached image directly");
    assert!(!fixture.cache.root.join(WRITE_FLAVOR).exists());
}

#[test]
fn concurrent_writers_publish_complete_pngs_and_remove_temporary_files() {
    let fixture = Fixture::new();
    fixture.image([200, 0, 0], UNIX_EPOCH + Duration::from_secs(10));
    std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.load().expect("first writer"));
        let second = scope.spawn(|| fixture.load().expect("second writer"));
        first.join().expect("first worker");
        second.join().expect("second worker");
    });
    assert!(
        read_png(&fixture.cached(), EDGE)
            .expect("published PNG")
            .is_some()
    );
    assert_eq!(
        fs::read_dir(fixture.cached().parent().expect("cache parent"))
            .expect("cache entries")
            .count(),
        1
    );
}

#[test]
fn external_rgb_cache_entries_decode_to_rgba() {
    let fixture = Fixture::new();
    fixture.image([200, 0, 0], UNIX_EPOCH + Duration::from_secs(10));
    let identity = fixture.identity();
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 512, 256);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .add_text_chunk("Thumb::URI".into(), identity.uri.clone())
        .expect("external URI");
    encoder
        .add_text_chunk("Thumb::MTime".into(), identity.mtime.to_string())
        .expect("external timestamp");
    let image = RgbImage::from_pixel(512, 256, Rgb([10, 20, 30]));
    let mut writer = encoder.write_header().expect("external RGB header");
    writer
        .write_image_data(image.as_raw())
        .expect("external RGB pixels");
    writer.finish().expect("external RGB PNG end");
    fixture
        .cache
        .write(
            &fixture.cache.root.join(WRITE_FLAVOR),
            &identity.filename(),
            &bytes,
        )
        .expect("cache RGB PNG");
    let loaded = fixture.load().expect("reused RGB cache entry");
    assert_eq!((loaded.width(), loaded.height()), (512, 256));
    assert_eq!(color(&loaded), [10, 20, 30, 255]);
}

#[test]
fn cache_writes_optional_mimetype_and_dimensions_and_invalidates_stale_entries() {
    let fixture = Fixture::new();
    fixture.image([120, 40, 200], UNIX_EPOCH + Duration::from_secs(10));
    let mut source = fixture.source();
    source.mime = Some(std::sync::Arc::from("image/bmp"));
    let thumb = load_with(source.thumbnail_target(), Some(&fixture.cache), || {
        source.generate()
    })
    .expect("generated thumbnail");
    assert_eq!((thumb.width(), thumb.height()), (512, 256));

    let cached = read_png(&fixture.cached(), EDGE)
        .expect("read cached PNG")
        .expect("saved PNG");
    assert_eq!(
        cached.attributes.get("Thumb::Mimetype").map(String::as_str),
        Some("image/bmp")
    );
    assert_eq!(
        cached
            .attributes
            .get("Thumb::Image::Width")
            .map(String::as_str),
        Some("1200")
    );
    assert_eq!(
        cached
            .attributes
            .get("Thumb::Image::Height")
            .map(String::as_str),
        Some("600")
    );

    // Also place a sibling flavor and failure marker, then invalidate_path must remove all of them.
    let identity = fixture.identity();
    let normal_path = fixture.cache.root.join("normal").join(identity.filename());
    private_directory(normal_path.parent().expect("normal dir")).expect("create normal dir");
    fs::write(&normal_path, b"placeholder").expect("write normal placeholder");
    let fail_path = fixture.cache.failure_path(&identity);
    private_directory(fail_path.parent().expect("fail dir")).expect("create fail dir");
    fs::write(&fail_path, b"placeholder").expect("write fail placeholder");

    fixture.cache.invalidate_path(&fixture.path);
    assert!(!fixture.cached().exists());
    assert!(!normal_path.exists());
    assert!(!fail_path.exists());
}

fn shared_png(width: u32, height: u32, color: [u8; 4], mtime: i128, uri: Option<&str>) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, Rgba(color));
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    if let Some(uri) = uri {
        encoder
            .add_text_chunk("Thumb::URI".into(), uri.to_owned())
            .expect("shared URI");
    }
    encoder
        .add_text_chunk("Thumb::MTime".into(), mtime.to_string())
        .expect("shared mtime");
    let mut writer = encoder.write_header().expect("shared header");
    writer
        .write_image_data(image.as_raw())
        .expect("shared pixels");
    writer.finish().expect("shared PNG end");
    bytes
}

fn write_bmp(path: &Path, color: [u8; 3], modified: SystemTime) {
    RgbImage::from_pixel(64, 32, Rgb(color))
        .save(path)
        .expect("write bmp fixture");
    File::open(path)
        .expect("open bmp fixture")
        .set_times(fs::FileTimes::new().set_modified(modified))
        .expect("set bmp timestamp");
}

#[test]
fn missing_external_thumbnailer_and_no_cover_do_not_persist_failure_markers() {
    let fixture = Fixture::new();
    let video_path = fixture.path.with_extension("mp4");
    fs::write(&video_path, b"dummy video payload").expect("write dummy video");
    let video_source = Source {
        path: video_path.clone(),
        kind: Kind::Video,
        mime: Some(std::sync::Arc::from("video/mp4")),
        mime_ancestors: Vec::new(),
        size: None,
        modified: None,
    };

    let err = load_with(
        video_source.thumbnail_target(),
        Some(&fixture.cache),
        || video_source.generate(),
    );
    assert!(matches!(err, Err(Error::NoCover)));

    let video_identity = Identity::new(
        &video_path,
        &fs::metadata(&video_path).expect("video metadata"),
    )
    .expect("video identity");
    let fail_path = fixture.cache.failure_path(&video_identity);
    assert!(
        !fail_path.exists(),
        "missing external thumbnailer must not persist a blocking fail/ marker"
    );

    // Even if a legacy X-GDS::Failure=no-cover marker exists on disk, lookup ignores it
    // so installing a thumbnailer later immediately succeeds.
    let legacy_bytes = encode(
        &RgbaImage::new(1, 1),
        Some(&video_identity),
        ExtraMeta::default(),
        Some("no-cover"),
    )
    .expect("encode legacy no-cover marker");
    fixture
        .cache
        .write(
            fail_path.parent().expect("fail parent"),
            &video_identity.filename(),
            &legacy_bytes,
        )
        .expect("write legacy no-cover marker");

    let generated = load_with(
        video_source.thumbnail_target(),
        Some(&fixture.cache),
        || {
            Ok(Generated::image_only(RgbaImage::from_pixel(
                64,
                32,
                Rgba([10, 180, 90, 255]),
            )))
        },
    )
    .expect("newly available thumbnailer succeeds despite legacy no-cover marker");
    assert_eq!(color(&generated), [10, 180, 90, 255]);
    assert!(!fail_path.exists());
}

#[cfg(unix)]
#[test]
fn cache_contains_detects_symlinked_cache_root_via_real_target_path() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().expect("symlink cache tempdir");
    let persist = directory.path().join("persist");
    let real_thumbs = persist.join("thumbnails");
    let real_normal = real_thumbs.join("normal");
    fs::create_dir_all(&real_normal).expect("create real thumbnail dir");

    let linked_cache = directory.path().join("home_cache");
    symlink(&persist, &linked_cache).expect("symlink cache dir");
    let cache = Cache {
        root: linked_cache.join("thumbnails"),
    };

    let real_cached = real_normal.join("cached.bmp");
    write_bmp(
        &real_cached,
        [200, 0, 0],
        UNIX_EPOCH + Duration::from_secs(10),
    );
    assert!(cache.contains(&real_cached));

    let source = Source {
        path: real_cached.clone(),
        kind: Kind::Image,
        mime: None,
        mime_ancestors: Vec::new(),
        size: None,
        modified: None,
    };
    load_with(source.thumbnail_target(), Some(&cache), || {
        source.generate()
    })
    .expect("display cached thumbnail via real path directly");
    assert!(
        !real_thumbs.join(WRITE_FLAVOR).exists(),
        "real target of symlinked cache root must not be thumbnailed into cache"
    );

    // Already-deleted files under the real cache target are still recognized as inside the cache.
    fs::remove_file(&real_cached).expect("remove real cached file");
    assert!(cache.contains(&real_cached));

    let outside = directory.path().join("outside.bmp");
    assert!(!cache.contains(&outside));
}

#[test]
fn directory_invalidation_cascades_to_descendants_without_following_symlinks() {
    let directory = tempfile::tempdir().expect("cascade tempdir");
    let cache = Cache {
        root: directory.path().join("thumbnails"),
    };
    let album = directory.path().join("album");
    let nested = album.join("nested");
    let outside = directory.path().join("outside");
    fs::create_dir_all(&nested).expect("create nested album");
    fs::create_dir_all(&outside).expect("create outside dir");

    let first = album.join("first.bmp");
    let second = nested.join("second.bmp");
    let external = outside.join("keep.bmp");
    let stamp = UNIX_EPOCH + Duration::from_secs(20);
    write_bmp(&first, [200, 20, 20], stamp);
    write_bmp(&second, [20, 200, 20], stamp);
    write_bmp(&external, [20, 20, 200], stamp);

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(&outside, album.join("link-outside")).expect("symlink outside dir");
        symlink(&album, nested.join("cycle")).expect("symlink cycle");
    }

    let first_src = Source {
        path: first.clone(),
        kind: Kind::Image,
        mime: None,
        mime_ancestors: Vec::new(),
        size: None,
        modified: None,
    };
    let second_src = Source {
        path: second.clone(),
        kind: Kind::Image,
        mime: None,
        mime_ancestors: Vec::new(),
        size: None,
        modified: None,
    };
    let external_src = Source {
        path: external.clone(),
        kind: Kind::Image,
        mime: None,
        mime_ancestors: Vec::new(),
        size: None,
        modified: None,
    };
    load_with(first_src.thumbnail_target(), Some(&cache), || {
        first_src.generate()
    })
    .expect("cache first");
    load_with(second_src.thumbnail_target(), Some(&cache), || {
        second_src.generate()
    })
    .expect("cache second");
    load_with(external_src.thumbnail_target(), Some(&cache), || {
        external_src.generate()
    })
    .expect("cache external");

    let first_cached = cache.root.join(WRITE_FLAVOR).join(
        Identity::new(&first, &fs::metadata(&first).expect("first meta"))
            .expect("first id")
            .filename(),
    );
    let second_cached = cache.root.join(WRITE_FLAVOR).join(
        Identity::new(&second, &fs::metadata(&second).expect("second meta"))
            .expect("second id")
            .filename(),
    );
    let external_cached = cache.root.join(WRITE_FLAVOR).join(
        Identity::new(&external, &fs::metadata(&external).expect("external meta"))
            .expect("external id")
            .filename(),
    );
    assert!(first_cached.is_file());
    assert!(second_cached.is_file());
    assert!(external_cached.is_file());

    let limited = ThumbnailInvalidation::capture_with_limit(&album, 1);
    assert_eq!(limited.paths().len(), 1);

    // Capture before renaming the directory, then invalidate old URIs after the rename succeeds.
    let plan = ThumbnailInvalidation::capture(&album);
    assert!(!plan.paths().contains(&external));
    let renamed = directory.path().join("album-renamed");
    fs::rename(&album, &renamed).expect("rename album");
    plan.invalidate_in(&cache);

    assert!(!first_cached.exists());
    assert!(!second_cached.exists());
    assert!(
        external_cached.exists(),
        "directory symlink targets outside the moved tree must not be invalidated"
    );
}

#[test]
fn shared_thumbnail_repository_loads_readonly_entries_and_validates_metadata() {
    let directory = tempfile::tempdir().expect("shared repo tempdir");
    let cache = Cache {
        root: directory.path().join("user-thumbnails"),
    };
    let album = directory.path().join("album");
    let shared_large = album.join(".sh_thumbnails").join("large");
    let shared_xlarge = album.join(".sh_thumbnails").join("x-large");
    fs::create_dir_all(&shared_large).expect("create shared large");
    fs::create_dir_all(&shared_xlarge).expect("create shared x-large");

    let photo = album.join("photo #1.bmp");
    let stamp = UNIX_EPOCH + Duration::from_secs(42);
    write_bmp(&photo, [200, 0, 0], stamp);
    let identity =
        Identity::new(&photo, &fs::metadata(&photo).expect("photo meta")).expect("photo identity");
    let expected_hash = format!("{:x}.png", Md5::digest(b"photo%20%231.bmp"));
    assert_eq!(
        identity.shared_filename().as_deref(),
        Some(expected_hash.as_str())
    );

    // 1. Omitted Thumb::URI with matching Thumb::MTime is served read-only without populating user cache.
    let large_entry = shared_large.join(&expected_hash);
    fs::write(
        &large_entry,
        shared_png(256, 128, [0, 180, 90, 255], identity.mtime, None),
    )
    .expect("write shared large entry");
    let source = Source {
        path: photo.clone(),
        kind: Kind::Image,
        mime: None,
        mime_ancestors: Vec::new(),
        size: None,
        modified: None,
    };
    let loaded = load_with(source.thumbnail_target(), Some(&cache), || {
        panic!("shared repository hit must not invoke generator")
    })
    .expect("load shared large thumbnail");
    assert_eq!((loaded.width(), loaded.height()), (256, 128));
    assert_eq!(color(&loaded), [0, 180, 90, 255]);
    assert!(!cache.root.exists());

    // 2. x-large takes precedence over large and accepts relative Thumb::URI.
    let xlarge_entry = shared_xlarge.join(&expected_hash);
    fs::write(
        &xlarge_entry,
        shared_png(
            512,
            256,
            [10, 60, 220, 255],
            identity.mtime,
            Some("photo%20%231.bmp"),
        ),
    )
    .expect("write shared x-large entry");
    let loaded_hi = load_with(source.thumbnail_target(), Some(&cache), || {
        panic!("shared x-large hit must not invoke generator")
    })
    .expect("load shared x-large thumbnail");
    assert_eq!((loaded_hi.width(), loaded_hi.height()), (512, 256));
    assert_eq!(color(&loaded_hi), [10, 60, 220, 255]);

    // 3. Wrong relative Thumb::URI is rejected (falls through to valid large entry).
    fs::write(
        &xlarge_entry,
        shared_png(
            512,
            256,
            [250, 250, 0, 255],
            identity.mtime,
            Some("other.bmp"),
        ),
    )
    .expect("write mismatched shared x-large entry");
    let fell_through = load_with(source.thumbnail_target(), Some(&cache), || {
        panic!("valid shared large entry should still match")
    })
    .expect("fallback to valid shared large");
    assert_eq!(color(&fell_through), [0, 180, 90, 255]);

    // 4. Stale Thumb::MTime is rejected and triggers normal generation.
    fs::remove_file(&xlarge_entry).expect("remove mismatched x-large");
    fs::write(
        &large_entry,
        shared_png(256, 128, [0, 180, 90, 255], identity.mtime - 1, None),
    )
    .expect("write stale shared large entry");
    let regenerated = load_with(source.thumbnail_target(), Some(&cache), || {
        source.generate()
    })
    .expect("regenerate after stale shared mtime");
    assert_eq!(color(&regenerated), [200, 0, 0, 255]);

    // 5. Files located inside .sh_thumbnails/ are displayed directly without writing to user cache.
    let inside_shared = Source {
        path: large_entry.clone(),
        kind: Kind::Image,
        mime: None,
        mime_ancestors: Vec::new(),
        size: None,
        modified: None,
    };
    let fresh_cache = Cache {
        root: directory.path().join("fresh-user-cache"),
    };
    load_with(inside_shared.thumbnail_target(), Some(&fresh_cache), || {
        inside_shared.generate()
    })
    .expect("display file inside .sh_thumbnails directly");
    assert!(!fresh_cache.root.exists());

    // 6. Non-image files without external thumbnailers can still reuse .sh_thumbnails/ via Source::load.
    let doc = album.join("manual.pdf");
    fs::write(&doc, b"%PDF-1.7\n").expect("write dummy pdf");
    let doc_id = Identity::new(&doc, &fs::metadata(&doc).expect("doc meta")).expect("doc id");
    let shared_normal = album.join(".sh_thumbnails").join("normal");
    fs::create_dir_all(&shared_normal).expect("create shared normal");
    fs::write(
        shared_normal.join(doc_id.shared_filename().expect("doc shared hash")),
        shared_png(128, 128, [90, 40, 170, 255], doc_id.mtime, None),
    )
    .expect("write shared doc thumbnail");
    let doc_source = Source {
        path: doc,
        kind: Kind::Other,
        mime: Some(std::sync::Arc::from("application/pdf")),
        mime_ancestors: Vec::new(),
        size: None,
        modified: None,
    };
    let inspection = doc_source.load(crate::service::file_inspection::Request::Thumbnail);
    assert!(matches!(
        inspection.thumbnail,
        crate::service::file_inspection::Field::Available(ref thumb)
            if color(thumb) == [90, 40, 170, 255]
    ));
}

#[test]
fn smaller_user_cache_flavors_are_not_upscaled_but_serve_as_generation_fallback() {
    let fixture = Fixture::new();
    fixture.image([200, 0, 0], UNIX_EPOCH + Duration::from_secs(10));
    let identity = fixture.identity();

    let normal_bytes = shared_png(
        128,
        64,
        [0, 120, 240, 255],
        identity.mtime,
        Some(&identity.uri),
    );
    fixture
        .cache
        .write(
            &fixture.cache.root.join("normal"),
            &identity.filename(),
            &normal_bytes,
        )
        .expect("write normal user cache entry");

    // When GDS can generate a crisp x-large thumbnail, it regenerates instead of upscaling normal.
    let crisp = fixture.load().expect("generate crisp x-large");
    assert_eq!((crisp.width(), crisp.height()), (512, 256));
    assert_eq!(color(&crisp), [200, 0, 0, 255]);

    // When generation is unavailable (NoCover), a valid smaller user-cache flavor is reused as fallback.
    fs::remove_file(fixture.cached()).expect("remove x-large");
    let large_bytes = shared_png(
        256,
        128,
        [30, 190, 70, 255],
        identity.mtime,
        Some(&identity.uri),
    );
    fixture
        .cache
        .write(
            &fixture.cache.root.join("large"),
            &identity.filename(),
            &large_bytes,
        )
        .expect("write large user cache entry");
    let fallback = load_with(
        fixture.source().thumbnail_target(),
        Some(&fixture.cache),
        || Err(Error::NoCover),
    )
    .expect("reuse large user-cache fallback");
    assert_eq!((fallback.width(), fallback.height()), (256, 128));
    assert_eq!(color(&fallback), [30, 190, 70, 255]);
    assert!(!fixture.cache.failure_path(&identity).exists());
}
