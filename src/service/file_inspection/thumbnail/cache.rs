//! The per-user freedesktop thumbnail cache.
//! https://specifications.freedesktop.org/thumbnail/latest-single/
use std::{
    collections::HashMap,
    fs::{self, File, Metadata},
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use directories::BaseDirs;
use image::RgbaImage;
use md5::{Digest, Md5};

use super::{EDGE, Error, Source, Thumbnail};

const FLAVORS: [(&str, u32); 4] = [
    ("normal", 128),
    ("large", 256),
    ("x-large", 512),
    ("xx-large", 1024),
];
const WRITE_FLAVOR: &str = "x-large";
const APPLICATION: &str = concat!("gds-", env!("CARGO_PKG_VERSION"));
const SOFTWARE: &str = concat!("GDS ", env!("CARGO_PKG_VERSION"));
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const NANOS_PER_SECOND: i128 = 1_000_000_000;

#[derive(Clone)]
pub(super) struct Cache {
    root: PathBuf,
}

struct Identity {
    uri: String,
    size: u64,
    modified: SystemTime,
    mtime: i128,
    nanos: i128,
}

enum Lookup {
    Miss,
    Ready(Thumbnail),
    Failed(Error),
}

struct CachedPng {
    bytes: Vec<u8>,
    attributes: HashMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
enum CacheError {
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

/// # Errors
/// Reports unreadable sources and thumbnail generation failures. Cache failures
/// are diagnosed separately and never prevent displaying a generated thumbnail.
pub(super) fn load(source: &Source, cache: Option<&Cache>) -> Result<Thumbnail, Error> {
    load_with(source, cache, || source.generate())
}

/// # Errors
/// Reports source access and generation failures; cache I/O is diagnosed separately.
pub(super) fn load_with(
    source: &Source,
    cache: Option<&Cache>,
    generate: impl FnOnce() -> Result<RgbaImage, Error>,
) -> Result<Thumbnail, Error> {
    // A cached preview must not bypass the original file's read permissions.
    let file = File::open(&source.path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::NotFile);
    }
    let identity = match Identity::new(&source.path, &metadata) {
        Ok(identity) => Some(identity),
        Err(error) => {
            tracing::debug!(%error, "thumbnail identity unavailable; generating without disk cache");
            None
        }
    };
    let cache = cache.filter(|cache| !cache.contains(&source.path));
    if let (Some(cache), Some(identity)) = (cache, identity.as_ref()) {
        match cache.lookup(identity) {
            Lookup::Ready(thumbnail) => return Ok(thumbnail),
            Lookup::Failed(error) => return Err(error),
            Lookup::Miss => {}
        }
    }
    match generate() {
        Ok(image) => {
            let bytes = encode(&image, identity.as_ref(), None)?;
            if let (Some(cache), Some(identity)) = (cache, identity.as_ref())
                && identity.unchanged(&source.path)
                && let Err(error) = cache.store(identity, &bytes)
            {
                tracing::warn!(%error, "cannot save thumbnail cache");
            }
            Ok(Thumbnail { bytes })
        }
        Err(error) => {
            // Missing tools, timeouts and I/O failures can resolve without the
            // document changing. Size limits are policy, not corrupt content.
            // Persist only content/format failures.
            if matches!(
                error,
                Error::Image(_) | Error::Svg(_) | Error::Video(_) | Error::NoCover
            ) && let (Some(cache), Some(identity)) = (cache, identity.as_ref())
                && identity.unchanged(&source.path)
                && let Err(cache_error) = cache.store_failure_kind(
                    identity,
                    if matches!(error, Error::NoCover) {
                        "no-cover"
                    } else {
                        "decode"
                    },
                )
            {
                tracing::warn!(%cache_error, "cannot save thumbnail failure marker");
            }
            Err(error)
        }
    }
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
        Ok(Self {
            uri: file_uri(path)?,
            size: metadata.len(),
            modified,
            mtime: nanos.div_euclid(NANOS_PER_SECOND),
            nanos,
        })
    }

    fn filename(&self) -> String {
        format!("{:x}.png", Md5::digest(self.uri.as_bytes()))
    }

    fn matches(&self, attributes: &HashMap<String, String>) -> bool {
        attributes.get("Thumb::URI") == Some(&self.uri)
            && attributes.get("Thumb::MTime").and_then(|value| value.parse().ok()) == Some(self.mtime)
            && attributes.get("Thumb::Size").is_none_or(|value| value.parse().ok() == Some(self.size))
            // The standard's seconds resolution cannot distinguish quick edits
            // of equally sized files. Verify our optional extension if present.
            && attributes.get("X-GDS::MTimeNS").is_none_or(|value| value.parse().ok() == Some(self.nanos))
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
        std::path::absolute(path).is_ok_and(|path| path.starts_with(&self.root))
    }

    fn failure_path(&self, identity: &Identity) -> PathBuf {
        self.root
            .join("fail")
            .join(APPLICATION)
            .join(identity.filename())
    }

    fn lookup(&self, identity: &Identity) -> Lookup {
        // Prefer the requested size, then larger interoperable cache entries.
        // Smaller entries are regenerated to avoid enlarging a low-res preview.
        for (flavor, edge) in FLAVORS.into_iter().filter(|(_, edge)| *edge >= EDGE) {
            if let Some(png) = self.read(&self.root.join(flavor).join(identity.filename()), edge)
                && identity.matches(&png.attributes)
            {
                return Lookup::Ready(Thumbnail { bytes: png.bytes });
            }
        }
        if let Some(png) = self.read(&self.failure_path(identity), 1)
            && identity.matches(&png.attributes)
            && png
                .attributes
                .get("X-GDS::Failure")
                .is_none_or(|value| value != "too-large")
        {
            return Lookup::Failed(
                if png
                    .attributes
                    .get("X-GDS::Failure")
                    .is_some_and(|kind| kind == "no-cover")
                {
                    Error::NoCover
                } else {
                    Error::CachedFailure
                },
            );
        }
        Lookup::Miss
    }

    fn read(&self, path: &Path, edge: u32) -> Option<CachedPng> {
        match read_png(path, edge) {
            Ok(png) => png,
            Err(error) => {
                tracing::debug!(path = %path.display(), %error, "ignoring unusable thumbnail cache entry");
                None
            }
        }
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

    /// # Errors
    /// Reports marker encoding and atomic write failures.
    fn store_failure_kind(&self, identity: &Identity, kind: &str) -> Result<(), CacheError> {
        let bytes = encode(&RgbaImage::new(1, 1), Some(identity), Some(kind))?;
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

/// # Errors
/// Reports invalid paths; file URIs preserve raw Unix filename bytes.
fn file_uri(path: &Path) -> Result<String, CacheError> {
    url::Url::from_file_path(std::path::absolute(path)?)
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
    let decoder = png::Decoder::new_with_limits(
        Cursor::new(&bytes),
        png::Limits {
            bytes: MAX_PNG_BYTES,
        },
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
    reader.next_frame(&mut vec![0; size])?;
    reader.finish()?;
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
    Ok(Some(CachedPng { bytes, attributes }))
}

fn relevant(key: &str) -> bool {
    matches!(
        key,
        "Thumb::URI"
            | "Thumb::MTime"
            | "Thumb::Size"
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
