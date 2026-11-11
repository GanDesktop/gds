//! Content type detection for the type column.
//!
//! File-name patterns and magic signatures come from the shared MIME database.
//! Application categories and short display names remain local presentation policy.
//! File names take precedence; only unknown names need a bounded content probe.
//! Detection never fails a directory listing.

use std::{ffi::OsStr, fs::File, io::Read, path::Path, sync::LazyLock};
use xdg_mime::SharedMimeInfo;

mod formats;

use super::details::{ContentCategory, ContentType};
use formats::Format;

/// Header bytes read for detection. Bounded so browsing never reads whole files.
const HEADER_LEN: u64 = 8 * 1024;

static MIME: LazyLock<SharedMimeInfo> = LazyLock::new(SharedMimeInfo::new);

impl ContentType {
    /// Detect the content type of a regular file.
    ///
    /// `name` is the name whose pattern is consulted; it is the target name for a
    /// symlink whose target is inspected.
    pub(super) fn detect(name: &OsStr, path: &Path) -> Option<Self> {
        Self::detect_with_database(&MIME, name, path)
    }

    fn detect_with_database(database: &SharedMimeInfo, name: &OsStr, path: &Path) -> Option<Self> {
        if let Some(mime) = database
            .get_mime_types_from_file_name(&name.to_string_lossy())
            .into_iter()
            .next()
            .map(|mime| database.unalias_mime_type(&mime).unwrap_or(mime))
            .filter(|mime| mime.essence_str() != "application/octet-stream")
        {
            return Some(Self::from_mime(mime.essence_str(), Some(name)));
        }

        let header = Self::read_header(path)?;
        let sniffed = (!header.is_empty())
            .then(|| database.get_mime_type_for_data(&header))
            .flatten()
            .map(|(mime, _)| database.unalias_mime_type(&mime).unwrap_or(mime));
        Some(match sniffed {
            Some(mime) => Self::from_mime(mime.essence_str(), None),
            None => Self::text_or_binary(&header),
        })
    }

    fn from_mime(mime: &str, name: Option<&OsStr>) -> Self {
        if let Some(format) = Format::for_mime(mime) {
            return Self::new(format.category, format.name.as_deref(), Some(mime));
        }
        let format = name
            .and_then(|name| Path::new(name).extension())
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_uppercase);
        Self::new(Self::category_for_mime(mime), format.as_deref(), Some(mime))
    }

    fn read_header(path: &Path) -> Option<Vec<u8>> {
        let file = File::open(path).ok()?;
        let mut header = Vec::new();
        let mut reader = file.take(HEADER_LEN);
        reader.read_to_end(&mut header).ok()?;
        Some(header)
    }

    fn category_for_mime(mime: &str) -> ContentCategory {
        match mime.split_once('/') {
            Some(("image", _)) => ContentCategory::Image,
            Some(("audio", _)) => ContentCategory::Audio,
            Some(("video", _)) => ContentCategory::Video,
            Some(("font", _)) => ContentCategory::Font,
            Some(("text", _)) => ContentCategory::Text,
            _ => ContentCategory::Binary,
        }
    }

    fn text_or_binary(header: &[u8]) -> Self {
        if header.is_empty() || Self::looks_like_text(header) {
            Self::new(ContentCategory::Text, None, Some("text/plain"))
        } else {
            Self::new(
                ContentCategory::Binary,
                None,
                Some("application/octet-stream"),
            )
        }
    }

    fn looks_like_text(bytes: &[u8]) -> bool {
        if bytes.contains(&0) {
            return false;
        }
        match std::str::from_utf8(bytes) {
            Ok(_) => true,
            Err(error) => {
                // A multi-byte sequence cut off by the read limit is still text.
                if error.valid_up_to() + 3 >= bytes.len() {
                    return true;
                }
                let control = bytes
                    .iter()
                    .copied()
                    .filter(|byte| {
                        byte.is_ascii_control()
                            && !matches!(byte, b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
                    })
                    .count();
                control * 100 <= bytes.len()
            }
        }
    }
}

#[cfg(test)]
mod tests;
