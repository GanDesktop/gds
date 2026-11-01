use super::*;
use crate::service::thumbnail::Kind;
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
            size: None,
            modified: None,
        }
    }

    fn load(&self) -> Result<Thumbnail, Error> {
        load(&self.source(), Some(&self.cache))
    }

    fn cached(&self) -> PathBuf {
        self.cache
            .root
            .join(WRITE_FLAVOR)
            .join(self.identity().filename())
    }
}

fn color(thumbnail: &Thumbnail) -> [u8; 4] {
    image::load_from_memory(&thumbnail.bytes)
        .expect("thumbnail PNG")
        .to_rgba8()
        .get_pixel(0, 0)
        .0
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
    let image = image::load_from_memory(&thumbnail.bytes).expect("generated PNG");
    assert_eq!((image.width(), image.height()), (512, 256));
    let cached = read_png(&fixture.cached(), EDGE)
        .expect("cache file")
        .expect("saved PNG");
    assert_eq!(cached.bytes, thumbnail.bytes);
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
