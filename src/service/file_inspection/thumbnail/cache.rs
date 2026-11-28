//! The per-user freedesktop thumbnail cache and read-only shared thumbnail repositories.
//! https://specifications.freedesktop.org/thumbnail/latest-single/
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, Metadata},
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use directories::BaseDirs;
use image::RgbaImage;
use md5::{Digest, Md5};

use super::{EDGE, Error, Generated, Target, Thumbnail};

/// Default upper bound on descendant paths collected when cascading thumbnail
/// invalidation across a directory tree.
pub const DEFAULT_MAX_THUMBNAIL_INVALIDATION_PATHS: usize = 4096;

const FLAVORS: [(&str, u32); 4] = [
    ("normal", 128),
    ("large", 256),
    ("x-large", 512),
    ("xx-large", 1024),
];
const SHARED_FLAVORS: [(&str, u32); 4] = [
    ("x-large", 512),
    ("xx-large", 1024),
    ("large", 256),
    ("normal", 128),
];
const WRITE_FLAVOR: &str = "x-large";
const SHARED_DIR_NAME: &str = ".sh_thumbnails";
const APPLICATION: &str = concat!("gds-", env!("CARGO_PKG_VERSION"));
const SOFTWARE: &str = concat!("GDS ", env!("CARGO_PKG_VERSION"));
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const NANOS_PER_SECOND: i128 = 1_000_000_000;

/// Pre-collected paths whose desktop thumbnail cache entries should be
/// invalidated once a destructive or relocating filesystem operation succeeds.
///
/// Capturing paths before the operation preserves the original `file://` URIs
/// for regular-file descendants inside moved, renamed, trashed, or deleted
/// directories without following directory symlinks.
#[derive(Clone, Debug, Default)]
pub struct ThumbnailInvalidation {
    paths: Vec<PathBuf>,
}

impl ThumbnailInvalidation {
    /// Capture `path` and, when `path` is a real directory (not a symlink),
    /// up to [`DEFAULT_MAX_THUMBNAIL_INVALIDATION_PATHS`] descendant paths
    /// without following directory symlinks.
    #[must_use]
    pub fn capture(path: &Path) -> Self {
        Self::capture_with_limit(path, DEFAULT_MAX_THUMBNAIL_INVALIDATION_PATHS)
    }

    /// Capture `path` and up to `limit` descendant paths for later invalidation.
    #[must_use]
    pub fn capture_with_limit(path: &Path, limit: usize) -> Self {
        Self {
            paths: collect_invalidation_paths(path, limit),
        }
    }

    /// Captured paths scheduled for invalidation.
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Remove cached desktop thumbnails and failure markers for all captured
    /// paths in the current user's cache (best effort).
    pub fn invalidate(self) {
        if self.paths.is_empty() {
            return;
        }
        if let Some(cache) = Cache::for_user() {
            self.invalidate_in(&cache);
        }
    }

    /// Remove cached thumbnails and failure markers for all captured paths in `cache`.
    pub(super) fn invalidate_in(&self, cache: &Cache) {
        cache.invalidate_paths(&self.paths);
    }
}

#[derive(Clone)]
pub(super) struct Cache {
    root: PathBuf,
}

struct Identity {
    uri: String,
    shared_dir: Option<PathBuf>,
    encoded_name: Option<String>,
    size: u64,
    modified: SystemTime,
    mtime: i128,
    nanos: i128,
}

struct CachedPng {
    image: RgbaImage,
    attributes: HashMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum CacheError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Decode(#[from] png::DecodingError),
    #[error(transparent)]
    Encode(#[from] png::EncodingError),
    #[error("cannot represent the file as a local URI")]
    Uri,
    #[error("cached thumbnail exceeds its size class or decoding limits")]
    Size,
}

/// Optional FreeDesktop thumbnail metadata written alongside mandatory `Thumb::URI`/`MTime`.
#[derive(Clone, Copy, Default)]
struct ExtraMeta<'a> {
    mime: Option<&'a str>,
    dimensions: Option<(u32, u32)>,
}

/// # Errors
/// Reports source access and generation failures; cache I/O is diagnosed separately.
pub(super) fn load_with(
    target: Target<'_>,
    cache: Option<&Cache>,
    generate: impl FnOnce() -> Result<Generated, Error>,
) -> Result<Thumbnail, Error> {
    // A cached preview must not bypass the original file's read permissions.
    let file = File::open(target.path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::NotFile);
    }
    let identity = match Identity::new(target.path, &metadata) {
        Ok(identity) => Some(identity),
        Err(error) => {
            tracing::debug!(%error, "thumbnail identity unavailable; generating without disk cache");
            None
        }
    };
    let is_thumbnail_file =
        is_in_shared_thumbnail_dir(target.path) || cache.is_some_and(|c| c.contains(target.path));
    let cache = if is_thumbnail_file { None } else { cache };
    if !is_thumbnail_file && let Some(identity) = identity.as_ref() {
        // 1. Prefer high-resolution user cache entries (>= EDGE) so GDS never
        // upscales a low-res user-cache preview when it can generate a crisp one.
        if let Some(cache) = cache
            && let Some(thumbnail) = cache.lookup_primary(identity)
        {
            return Ok(thumbnail);
        }
        // 2. Check read-only FreeDesktop shared repositories (<dir>/.sh_thumbnails/).
        if let Some(thumbnail) = lookup_shared(identity) {
            return Ok(thumbnail);
        }
        // 3. Respect per-application content/decoder failure markers unless another
        // application already populated a valid smaller interoperable user-cache flavor.
        if let Some(cache) = cache
            && cache.has_failure_marker(identity)
        {
            if let Some(thumbnail) = cache.lookup_fallback(identity) {
                return Ok(thumbnail);
            }
            return Err(Error::CachedFailure);
        }
    }
    match generate() {
        Ok(generated) => {
            if let (Some(cache), Some(identity)) = (cache, identity.as_ref())
                && identity.unchanged(target.path)
            {
                let extra = ExtraMeta {
                    mime: target.mime,
                    dimensions: generated.source_dimensions,
                };
                match encode(&generated.image, Some(identity), extra, None) {
                    Ok(bytes) => {
                        if let Err(error) = cache.store(identity, &bytes) {
                            tracing::warn!(%error, "cannot save thumbnail cache");
                        }
                    }
                    Err(error) => tracing::warn!(%error, "cannot encode thumbnail cache"),
                }
            }
            Ok(Thumbnail::new(generated.image))
        }
        Err(error) => {
            if let (Some(cache), Some(identity)) = (cache, identity.as_ref()) {
                // When generation fails or no thumbnailer is installed, reuse any valid
                // smaller interoperable user-cache flavor (large/normal) before failing.
                if let Some(thumbnail) = cache.lookup_fallback(identity) {
                    return Ok(thumbnail);
                }
                // Missing tools, missing external thumbnailers, absent embedded cover art,
                // timeouts, and I/O failures can resolve without the document changing.
                // Size limits are policy, not corrupt content. Persist only content/decoder
                // failures per the FreeDesktop Thumbnail Managing Standard.
                if matches!(
                    error,
                    Error::Image(_) | Error::Svg(_) | Error::ExternalFailed(_)
                ) && identity.unchanged(target.path)
                    && let Err(cache_error) = cache.store_failure(identity)
                {
                    tracing::warn!(%cache_error, "cannot save thumbnail failure marker");
                }
            }
            Err(error)
        }
    }
}

/// Best-effort read-only lookup in a FreeDesktop shared `.sh_thumbnails/` repository
/// next to `target.path`, enforcing read permissions on the original file.
pub(super) fn lookup_shared_for_target(target: Target<'_>) -> Option<Thumbnail> {
    if is_in_shared_thumbnail_dir(target.path) {
        return None;
    }
    let abs = std::path::absolute(target.path).ok()?;
    if !abs.parent()?.join(SHARED_DIR_NAME).is_dir() {
        return None;
    }
    let file = File::open(target.path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    let identity = Identity::new(target.path, &metadata).ok()?;
    lookup_shared(&identity)
}

impl Identity {
    /// # Errors
    /// Reports inaccessible metadata or an unrepresentable file URI.
    fn new(path: &Path, metadata: &Metadata) -> Result<Self, CacheError> {
        let modified = metadata.modified()?;
        let nanos = match modified.duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_nanos() as i128,
            Err(error) => -(error.duration().as_nanos() as i128),
        };
        let absolute = std::path::absolute(path)?;
        let uri = file_uri_from_absolute(&absolute)?;
        let shared_dir = absolute.parent().map(|parent| parent.join(SHARED_DIR_NAME));
        let encoded_name = uri
            .rsplit_once('/')
            .map(|(_, name)| name.to_owned())
            .filter(|name| !name.is_empty());
        Ok(Self {
            uri,
            shared_dir,
            encoded_name,
            size: metadata.len(),
            modified,
            mtime: nanos.div_euclid(NANOS_PER_SECOND),
            nanos,
        })
    }

    fn filename(&self) -> String {
        format!("{:x}.png", Md5::digest(self.uri.as_bytes()))
    }

    fn shared_filename(&self) -> Option<String> {
        self.encoded_name
            .as_deref()
            .map(|name| format!("{:x}.png", Md5::digest(name.as_bytes())))
    }

    fn matches(&self, attributes: &HashMap<String, String>) -> bool {
        attributes.get("Thumb::URI") == Some(&self.uri)
            && self.matches_timestamps_and_size(attributes)
    }

    fn matches_shared(&self, attributes: &HashMap<String, String>) -> bool {
        let Some(expected_name) = self.encoded_name.as_deref() else {
            return false;
        };
        // In FreeDesktop shared repositories (.sh_thumbnails/), Thumb::URI is omitted
        // or holds the percent-encoded relative filename without a directory prefix.
        let uri_ok = match attributes.get("Thumb::URI") {
            None => true,
            Some(uri) => uri == expected_name || uri.strip_prefix("./") == Some(expected_name),
        };
        uri_ok && self.matches_timestamps_and_size(attributes)
    }

    fn matches_timestamps_and_size(&self, attributes: &HashMap<String, String>) -> bool {
        attributes
            .get("Thumb::MTime")
            .and_then(|value| value.parse().ok())
            == Some(self.mtime)
            && attributes
                .get("Thumb::Size")
                .is_none_or(|value| value.parse().ok() == Some(self.size))
            // The standard's seconds resolution cannot distinguish quick edits
            // of equally sized files. Verify our optional extension if present.
            && attributes
                .get("X-GDS::MTimeNS")
                .is_none_or(|value| value.parse().ok() == Some(self.nanos))
    }

    fn unchanged(&self, path: &Path) -> bool {
        fs::metadata(path).is_ok_and(|metadata| {
            metadata.is_file()
                && metadata.len() == self.size
                && metadata.modified().ok() == Some(self.modified)
        })
    }
}

impl Cache {
    pub(super) fn for_user() -> Option<Self> {
        BaseDirs::new().map(|dirs| Self {
            root: dirs.cache_dir().join("thumbnails"),
        })
    }

    fn contains(&self, path: &Path) -> bool {
        let abs_root = std::path::absolute(&self.root).ok();
        let canon_root = resolve_prefix(&self.root);
        Self::contains_with_roots(path, abs_root.as_deref(), canon_root.as_deref())
    }

    fn contains_with_roots(
        path: &Path,
        abs_root: Option<&Path>,
        canon_root: Option<&Path>,
    ) -> bool {
        if let Some(root) = abs_root
            && std::path::absolute(path).is_ok_and(|abs_path| abs_path.starts_with(root))
        {
            return true;
        }
        let Some(root) = canon_root else {
            return false;
        };
        resolve_prefix(path).is_some_and(|canon_path| canon_path.starts_with(root))
    }

    fn failure_path(&self, identity: &Identity) -> PathBuf {
        self.root
            .join("fail")
            .join(APPLICATION)
            .join(identity.filename())
    }

    fn lookup_primary(&self, identity: &Identity) -> Option<Thumbnail> {
        // Prefer the requested size, then larger interoperable cache entries.
        // Smaller entries are regenerated when possible to avoid enlarging a low-res preview.
        let filename = identity.filename();
        for (flavor, edge) in FLAVORS.into_iter().filter(|(_, edge)| *edge >= EDGE) {
            if let Some(png) = read_cached_png(&self.root.join(flavor).join(&filename), edge)
                && identity.matches(&png.attributes)
            {
                return Some(Thumbnail::new(png.image));
            }
        }
        None
    }

    fn lookup_fallback(&self, identity: &Identity) -> Option<Thumbnail> {
        let filename = identity.filename();
        for (flavor, edge) in FLAVORS.into_iter().rev().filter(|(_, edge)| *edge < EDGE) {
            if let Some(png) = read_cached_png(&self.root.join(flavor).join(&filename), edge)
                && identity.matches(&png.attributes)
            {
                return Some(Thumbnail::new(png.image));
            }
        }
        None
    }

    fn has_failure_marker(&self, identity: &Identity) -> bool {
        let Some(png) = read_cached_png(&self.failure_path(identity), 1) else {
            return false;
        };
        identity.matches(&png.attributes)
            && png
                .attributes
                .get("X-GDS::Failure")
                .is_none_or(|value| !matches!(value.as_str(), "too-large" | "no-cover"))
    }

    /// # Errors
    /// Reports cache directory and atomic replacement failures.
    fn store(&self, identity: &Identity, bytes: &[u8]) -> Result<(), CacheError> {
        self.write(&self.root.join(WRITE_FLAVOR), &identity.filename(), bytes)?;
        match fs::remove_file(self.failure_path(identity)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => tracing::debug!(%error, "cannot remove old thumbnail failure marker"),
        }
        Ok(())
    }

    /// Remove cached thumbnail PNGs and failure markers for `path` across all standard sizes
    /// after deletion, trashing, moving, or renaming (FreeDesktop Thumbnail Managing Standard).
    pub(super) fn invalidate_path(&self, path: &Path) {
        let abs_root = std::path::absolute(&self.root).ok();
        let canon_root = resolve_prefix(&self.root);
        self.invalidate_path_with_roots(path, abs_root.as_deref(), canon_root.as_deref());
    }

    fn invalidate_paths(&self, paths: &[PathBuf]) {
        let abs_root = std::path::absolute(&self.root).ok();
        let canon_root = resolve_prefix(&self.root);
        for path in paths {
            self.invalidate_path_with_roots(path, abs_root.as_deref(), canon_root.as_deref());
        }
    }

    fn invalidate_path_with_roots(
        &self,
        path: &Path,
        abs_root: Option<&Path>,
        canon_root: Option<&Path>,
    ) {
        if Self::contains_with_roots(path, abs_root, canon_root) {
            return;
        }
        let Ok(uri) = file_uri(path) else {
            return;
        };
        let filename = format!("{:x}.png", Md5::digest(uri.as_bytes()));
        for (flavor, _) in FLAVORS {
            let entry = self.root.join(flavor).join(&filename);
            if let Err(error) = fs::remove_file(&entry)
                && error.kind() != io::ErrorKind::NotFound
            {
                tracing::debug!(path = %entry.display(), %error, "cannot remove stale thumbnail cache entry");
            }
        }
        let failure = self.root.join("fail").join(APPLICATION).join(&filename);
        if let Err(error) = fs::remove_file(&failure)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::debug!(path = %failure.display(), %error, "cannot remove stale thumbnail failure marker");
        }
    }

    /// # Errors
    /// Reports marker encoding and atomic write failures.
    fn store_failure(&self, identity: &Identity) -> Result<(), CacheError> {
        let bytes = encode(
            &RgbaImage::new(1, 1),
            Some(identity),
            ExtraMeta::default(),
            Some("decode"),
        )?;
        private_directory(&self.root)?;
        private_directory(&self.root.join("fail"))?;
        self.write(
            &self.root.join("fail").join(APPLICATION),
            &identity.filename(),
            &bytes,
        )
    }

    /// # Errors
    /// Reports directory creation, permission, write or rename failures.
    fn write(&self, directory: &Path, filename: &str, bytes: &[u8]) -> Result<(), CacheError> {
        private_directory(&self.root)?;
        private_directory(directory)?;
        // NamedTempFile creates mode 0600. Same-directory rename publishes a
        // complete PNG atomically, including when other thumbnailers are active.
        let mut temporary = tempfile::Builder::new()
            .prefix(".gds-")
            .tempfile_in(directory)?;
        temporary.write_all(bytes)?;
        temporary
            .persist(directory.join(filename))
            .map_err(|error| error.error)?;
        Ok(())
    }
}

fn lookup_shared(identity: &Identity) -> Option<Thumbnail> {
    let shared_dir = identity.shared_dir.as_deref()?;
    if !shared_dir.is_dir() {
        return None;
    }
    let filename = identity.shared_filename()?;
    for (flavor, edge) in SHARED_FLAVORS {
        let path = shared_dir.join(flavor).join(&filename);
        if let Some(png) = read_cached_png(&path, edge)
            && identity.matches_shared(&png.attributes)
        {
            return Some(Thumbnail::new(png.image));
        }
    }
    None
}

fn read_cached_png(path: &Path, edge: u32) -> Option<CachedPng> {
    match read_png(path, edge) {
        Ok(png) => png,
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "ignoring unusable thumbnail cache entry");
            None
        }
    }
}

fn is_in_shared_thumbnail_dir(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == SHARED_DIR_NAME)
}

/// Resolve symlinks on `path` (or its deepest existing ancestor when `path`
/// does not exist yet or was already deleted), falling back to `std::path::absolute`.
fn resolve_prefix(path: &Path) -> Option<PathBuf> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Some(canonical);
    }
    let absolute = std::path::absolute(path).ok()?;
    let mut current = absolute.as_path();
    let mut tail = Vec::new();
    while let Some(name) = current.file_name() {
        tail.push(name);
        let Some(parent) = current.parent() else {
            break;
        };
        if let Ok(mut canonical) = fs::canonicalize(parent) {
            for component in tail.into_iter().rev() {
                canonical.push(component);
            }
            return Some(canonical);
        }
        current = parent;
    }
    Some(absolute)
}

fn collect_invalidation_paths(path: &Path, limit: usize) -> Vec<PathBuf> {
    if limit == 0 {
        return Vec::new();
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return vec![path.to_path_buf()];
    };
    if !metadata.file_type().is_dir() {
        return vec![path.to_path_buf()];
    }
    let mut paths = Vec::new();
    let mut dirs = VecDeque::from([path.to_path_buf()]);
    let mut visited_dirs = 0usize;
    while let Some(dir) = dirs.pop_front() {
        if visited_dirs >= limit {
            break;
        }
        visited_dirs += 1;
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let entry_path = entry.path();
            if file_type.is_symlink() {
                // Symlinks to files may have cached URIs of their own; directory symlinks
                // are never traversed so cycles and external trees stay untouched.
                paths.push(entry_path);
            } else if file_type.is_dir() {
                dirs.push_back(entry_path);
            } else if file_type.is_file() {
                paths.push(entry_path);
            }
            if paths.len() >= limit {
                return paths;
            }
        }
    }
    paths
}

/// # Errors
/// Reports invalid paths; file URIs preserve raw Unix filename bytes.
pub(super) fn file_uri(path: &Path) -> Result<String, CacheError> {
    file_uri_from_absolute(&std::path::absolute(path)?)
}

fn file_uri_from_absolute(path: &Path) -> Result<String, CacheError> {
    url::Url::from_file_path(path)
        .map(String::from)
        .map_err(|_| CacheError::Uri)
}

/// # Errors
/// Reports failures creating private cache directories.
fn private_directory(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        builder.mode(0o700).create(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    builder.create(path)?;
    Ok(())
}

/// # Errors
/// Reports malformed, incomplete, or excessively large cached PNGs.
fn read_png(path: &Path, edge: u32) -> Result<Option<CachedPng>, CacheError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if file.metadata()?.len() > MAX_PNG_BYTES as u64 {
        return Err(CacheError::Size);
    }
    let mut bytes = Vec::new();
    file.take(MAX_PNG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PNG_BYTES {
        return Err(CacheError::Size);
    }
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(&bytes),
        png::Limits {
            bytes: MAX_PNG_BYTES,
        },
    );
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::ALPHA | png::Transformations::STRIP_16,
    );
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    if info.width > edge
        || info.height > edge
        || info.bit_depth != png::BitDepth::Eight
        || info.interlaced
    {
        return Err(CacheError::Size);
    }
    let size = reader
        .output_buffer_size()
        .filter(|size| *size <= MAX_PNG_BYTES)
        .ok_or(CacheError::Size)?;
    let mut pixels = vec![0; size];
    let frame = reader.next_frame(&mut pixels)?;
    reader.finish()?;
    let image = rgba_from_frame(frame, pixels)?;
    let info = reader.info();
    let mut attributes = HashMap::new();
    for text in &info.uncompressed_latin1_text {
        if relevant(&text.keyword) {
            attributes.insert(text.keyword.clone(), text.text.clone());
        }
    }
    for mut text in info.compressed_latin1_text.clone() {
        if relevant(&text.keyword) {
            text.decompress_text_with_limit(MAX_TEXT_BYTES)?;
            attributes.insert(text.keyword.clone(), text.get_text()?);
        }
    }
    for mut text in info.utf8_text.clone() {
        if relevant(&text.keyword) {
            text.decompress_text_with_limit(MAX_TEXT_BYTES)?;
            attributes.insert(text.keyword.clone(), text.get_text()?);
        }
    }
    Ok(Some(CachedPng { image, attributes }))
}

fn rgba_from_frame(frame: png::OutputInfo, mut pixels: Vec<u8>) -> Result<RgbaImage, CacheError> {
    pixels.truncate(frame.buffer_size());
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(CacheError::Size);
    }
    let width = frame.width;
    let height = frame.height;
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or(CacheError::Size)?;
    match frame.color_type {
        png::ColorType::Rgba => RgbaImage::from_raw(width, height, pixels),
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity(pixel_count);
            for &[r, g, b] in pixels.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[r, g, b, u8::MAX]);
            }
            RgbaImage::from_raw(width, height, rgba)
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity(pixel_count);
            for &gray in &pixels {
                rgba.extend_from_slice(&[gray, gray, gray, u8::MAX]);
            }
            RgbaImage::from_raw(width, height, rgba)
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity(pixel_count);
            for &[gray, alpha] in pixels.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[gray, gray, gray, alpha]);
            }
            RgbaImage::from_raw(width, height, rgba)
        }
        png::ColorType::Indexed => None,
    }
    .ok_or(CacheError::Size)
}

/// Best-effort removal of cached thumbnails for `path` in the current user's desktop cache.
pub(super) fn invalidate_user_path(path: &Path) {
    if let Some(cache) = Cache::for_user() {
        cache.invalidate_path(path);
    }
}

fn relevant(key: &str) -> bool {
    matches!(
        key,
        "Thumb::URI"
            | "Thumb::MTime"
            | "Thumb::Size"
            | "Thumb::Mimetype"
            | "Thumb::Image::Width"
            | "Thumb::Image::Height"
            | "Software"
            | "X-GDS::MTimeNS"
            | "X-GDS::Failure"
    )
}

/// # Errors
/// Reports PNG encoding errors.
fn encode(
    image: &RgbaImage,
    identity: Option<&Identity>,
    extra: ExtraMeta<'_>,
    failure: Option<&str>,
) -> Result<Vec<u8>, png::EncodingError> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.add_text_chunk("Software".into(), SOFTWARE.into())?;
    if let Some(identity) = identity {
        for (key, value) in [
            ("Thumb::URI", identity.uri.clone()),
            ("Thumb::MTime", identity.mtime.to_string()),
            ("Thumb::Size", identity.size.to_string()),
            ("X-GDS::MTimeNS", identity.nanos.to_string()),
        ] {
            encoder.add_text_chunk(key.into(), value)?;
        }
    }
    if let Some(mime) = extra.mime.filter(|m| !m.is_empty()) {
        encoder.add_text_chunk("Thumb::Mimetype".into(), mime.to_owned())?;
    }
    if let Some((width, height)) = extra.dimensions {
        encoder.add_text_chunk("Thumb::Image::Width".into(), width.to_string())?;
        encoder.add_text_chunk("Thumb::Image::Height".into(), height.to_string())?;
    }
    if let Some(failure) = failure {
        encoder.add_text_chunk("X-GDS::Failure".into(), failure.into())?;
    }
    let mut writer = encoder.write_header()?;
    writer.write_image_data(image.as_raw())?;
    writer.finish()?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
