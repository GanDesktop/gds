//! Content type detection, category derivation, and localized description lookup.
//!
//! Filename rules, aliases, subclasses, generic icon hints, and magic signatures
//! come from Linux `shared-mime-info` (preferring binary `mime.cache` with
//! plain-text fallback). Generated GitHub Linguist tables refine source-code
//! classification and cover modern developer extensions without disk I/O.
//! Unique non-shadowed matches, parent+subclass-only matches (e.g. `.json`,
//! `.opus`), and Linguist C0 code matches complete purely in memory; conflicts
//! delegated to [`disambiguation`] and unmatched filenames use a bounded 8 KiB
//! content probe when enrichment is enabled.

pub(crate) mod descriptions;
mod disambiguation;
mod formats;
mod linguist;
mod mime_db;

use std::{
    ffi::OsStr,
    fs::File,
    io::Read,
    path::Path,
    sync::{Arc, LazyLock},
};

use super::details::{ContentCategory, ContentType, DetectionSource};
use disambiguation::{ConflictKind, Outcome};
use linguist::LinguistFallback;
use mime_db::MimeDatabase;

/// Header bytes read for detection. Bounded so browsing never reads whole files.
const HEADER_LEN: u64 = 8 * 1024;

static MIME_DB: LazyLock<MimeDatabase> = LazyLock::new(MimeDatabase::from_xdg);
static TEXT_PLAIN_MIME: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from("text/plain"));
static OCTET_STREAM_MIME: LazyLock<Arc<str>> =
    LazyLock::new(|| Arc::from("application/octet-stream"));

/// Result of zero-I/O filename classification.
pub(super) enum FilenameClassification {
    /// Uniquely resolved without needing disk I/O (Branch A, B0, or C0).
    Resolved(ContentType),
    /// Conflict requiring bounded 8 KiB structural disambiguation (Branch B1);
    /// carries a deterministic fallback for immediate display or when probing is off.
    Ambiguous(ContentType),
    /// No filename rule matched (Branch C); needs content probing when enabled.
    Unmatched,
}

impl FilenameClassification {
    pub(super) fn into_content(self) -> Option<ContentType> {
        match self {
            Self::Resolved(content) | Self::Ambiguous(content) => Some(content),
            Self::Unmatched => None,
        }
    }

    pub(super) fn into_parts(self) -> (Option<ContentType>, bool) {
        match self {
            Self::Resolved(content) => (Some(content), true),
            Self::Ambiguous(fallback) => (Some(fallback), false),
            Self::Unmatched => (None, false),
        }
    }
}

impl ContentType {
    /// Full content detection for a single regular file (used by `Entry::inspect`
    /// and Properties). Preloads the localized XML description off the UI thread.
    pub(super) fn detect(name: &OsStr, path: &Path) -> Option<Self> {
        let detected = Self::detect_with_database(&MIME_DB, name, path);
        if let Some(mime) = detected.as_ref().and_then(|c| c.mime.as_deref()) {
            descriptions::preload_mime(mime);
        }
        detected
    }

    /// Zero-I/O filename classification distinguishing resolved, ambiguous, and unmatched entries.
    pub(super) fn classify_filename(name: &OsStr) -> FilenameClassification {
        Self::classify_filename_with_database(&MIME_DB, name)
    }

    /// Zero-I/O content classification for callers that never perform content probing
    /// (e.g. remote directories or trash listings).
    pub(super) fn detect_from_name(name: &OsStr) -> Option<Self> {
        Self::classify_filename_with_database(&MIME_DB, name).into_content()
    }

    /// Bounded 8 KiB content probe for ambiguous or unmatched regular files during
    /// background enrichment. Also preloads the resolved MIME's XML description.
    pub(super) fn probe_content(name: &OsStr, path: &Path) -> Option<Self> {
        let detected = Self::probe_content_with_database(&MIME_DB, name, path);
        if let Some(mime) = detected.as_ref().and_then(|c| c.mime.as_deref()) {
            descriptions::preload_mime(mime);
        }
        detected
    }

    fn detect_with_database(db: &MimeDatabase, name: &OsStr, path: &Path) -> Option<Self> {
        match Self::classify_filename_with_database(db, name) {
            FilenameClassification::Resolved(content) => Some(content),
            FilenameClassification::Ambiguous(fallback) => {
                Self::probe_content_with_database(db, name, path).or(Some(fallback))
            }
            FilenameClassification::Unmatched => Self::probe_content_with_database(db, name, path),
        }
    }

    fn classify_filename_with_database(db: &MimeDatabase, name: &OsStr) -> FilenameClassification {
        let candidates = db.filename_candidates(&name.to_string_lossy());
        if candidates.is_empty() {
            // Branch C0: zero-I/O Linguist code language fallback (e.g. `.zig`, `.nix`, `Dockerfile`).
            return if let Some(language) = linguist::code_language_for_filename(name) {
                FilenameClassification::Resolved(Self::from_linguist(language))
            } else {
                FilenameClassification::Unmatched
            };
        }

        match disambiguation::detect_conflict(db, name, &candidates) {
            ConflictKind::None => {
                // Branch A (unique non-shadowed MIME) or B0 (parent + subclasses collapsed to base).
                let resolved = disambiguation::converge_candidates(db, &candidates);
                FilenameClassification::Resolved(Self::build_from_mime(
                    db,
                    &resolved,
                    Some(name),
                    DetectionSource::Filename,
                ))
            }
            ConflictKind::MultiMime | ConflictKind::LinguistShadow => {
                // Branch B1: multi-MIME or binary/XML shadow conflict pending 8 KiB disambiguation.
                let fallback = disambiguation::converge_candidates(db, &candidates);
                FilenameClassification::Ambiguous(Self::build_from_mime(
                    db,
                    &fallback,
                    Some(name),
                    DetectionSource::Filename,
                ))
            }
        }
    }

    fn probe_content_with_database(db: &MimeDatabase, name: &OsStr, path: &Path) -> Option<Self> {
        let candidates = db.filename_candidates(&name.to_string_lossy());
        if !candidates.is_empty()
            && disambiguation::detect_conflict(db, name, &candidates) != ConflictKind::None
        {
            let header = Self::read_header(path);
            let outcome =
                disambiguation::resolve_with_header(db, name, &candidates, header.as_deref());
            return Some(match outcome {
                Outcome::Mime { mime, source } => {
                    Self::build_from_mime(db, &mime, Some(name), source)
                }
                Outcome::Linguist(LinguistFallback::Code(language)) => {
                    Self::from_linguist(language)
                }
                Outcome::Linguist(LinguistFallback::Data(language)) => {
                    Self::from_linguist_data(language)
                }
            });
        }

        // Branch C: unmatched filename — read up to 8 KiB.
        let header = Self::read_header(path)?;
        if header.is_empty() {
            return Some(Self::from_mime(
                ContentCategory::Text,
                TEXT_PLAIN_MIME.clone(),
                DetectionSource::Heuristic,
                true,
            ));
        }
        if let Some((raw_mime, _)) = db.sniff_data(&header) {
            let mime = disambiguation::sanitize_disguised_desktop(name, raw_mime);
            return Some(Self::build_from_mime(
                db,
                &mime,
                Some(name),
                DetectionSource::Magic,
            ));
        }
        Some(Self::text_or_binary(&header))
    }

    fn build_from_mime(
        db: &MimeDatabase,
        mime: &str,
        name: Option<&OsStr>,
        source: DetectionSource,
    ) -> Self {
        let category = Self::category_for_mime(db, mime, name);
        let text_previewable = matches!(category, ContentCategory::Text | ContentCategory::Code)
            || (category == ContentCategory::Document && db.is_subclass(mime, "text/plain"));
        let interned = match mime {
            "text/plain" => TEXT_PLAIN_MIME.clone(),
            "application/octet-stream" => OCTET_STREAM_MIME.clone(),
            other => Arc::from(other),
        };
        Self::from_mime(category, interned, source, text_previewable)
    }

    /// Stage 2 decision tree: derive `ContentCategory` from `shared-mime-info`
    /// (`generic-icons`, MIME prefix, `subclasses`), Linguist `Code` refinement,
    /// and the minimal last-resort patch table.
    fn category_for_mime(db: &MimeDatabase, mime: &str, name: Option<&OsStr>) -> ContentCategory {
        // 1. Own explicit generic-icon hint in `shared-mime-info`.
        if let Some(icon) = db.explicit_generic_icon(mime)
            && let Some(category) = map_generic_icon(&icon, mime, name)
        {
            return category;
        }

        // 2. Top-level MIME prefix (evaluated before `subclasses` so e.g. `text/x-python`
        // does not inherit `application-x-executable` from its parent list).
        if let Some((top, _)) = mime.split_once('/') {
            match top {
                "image" => return ContentCategory::Image,
                "audio" => return ContentCategory::Audio,
                "video" => return ContentCategory::Video,
                "font" => return ContentCategory::Font,
                "text" => {
                    return if linguist::is_code_text(mime, name) {
                        ContentCategory::Code
                    } else {
                        ContentCategory::Text
                    };
                }
                _ => {}
            }
        }

        // 3. `subclasses` inheritance (parent generic-icon, then `text/plain` subclass).
        if let Some(parent_icon) = db.inherited_generic_icon(mime)
            && let Some(category) = map_generic_icon(&parent_icon, mime, name)
        {
            return category;
        }
        if db.is_subclass(mime, "text/plain") {
            return if linguist::is_code_text(mime, name) {
                ContentCategory::Code
            } else {
                ContentCategory::Text
            };
        }

        // 4. Minimal last-resort patch table.
        if let Some(patched) = formats::patch_category(mime) {
            return patched;
        }

        // 5. Fallback.
        ContentCategory::Binary
    }

    fn read_header(path: &Path) -> Option<Vec<u8>> {
        let file = File::open(path).ok()?;
        let mut header = Vec::new();
        let mut reader = file.take(HEADER_LEN);
        reader.read_to_end(&mut header).ok()?;
        Some(header)
    }

    fn text_or_binary(header: &[u8]) -> Self {
        if header.is_empty() || disambiguation::looks_like_text(header) {
            Self::from_mime(
                ContentCategory::Text,
                TEXT_PLAIN_MIME.clone(),
                DetectionSource::Heuristic,
                true,
            )
        } else {
            Self::from_mime(
                ContentCategory::Binary,
                OCTET_STREAM_MIME.clone(),
                DetectionSource::Heuristic,
                false,
            )
        }
    }
}

/// Match a file's basename (or already-detected MIME) against expected picker MIME patterns
/// using the process-wide shared MIME database.
pub(crate) fn matches_mime_filter<'a>(
    name: &OsStr,
    detected_mime: Option<&str>,
    expected: impl IntoIterator<Item = &'a str>,
) -> bool {
    matches_mime_filter_with_database(&MIME_DB, name, detected_mime, expected)
}

fn matches_mime_filter_with_database<'a>(
    db: &MimeDatabase,
    name: &OsStr,
    detected_mime: Option<&str>,
    expected: impl IntoIterator<Item = &'a str>,
) -> bool {
    let resolved: std::borrow::Cow<'_, str> = if let Some(mime) = detected_mime
        .map(|raw| raw.split(';').next().unwrap_or(raw).trim())
        .filter(|essence| essence.contains('/'))
    {
        std::borrow::Cow::Borrowed(mime)
    } else {
        let candidates = db.filename_candidates(&name.to_string_lossy());
        if candidates.is_empty() {
            std::borrow::Cow::Borrowed("application/octet-stream")
        } else {
            std::borrow::Cow::Owned(disambiguation::converge_candidates(db, &candidates))
        }
    };

    expected
        .into_iter()
        .any(|pattern| db.is_subclass(resolved.as_ref(), pattern))
}

fn map_generic_icon(icon: &str, mime: &str, name: Option<&OsStr>) -> Option<ContentCategory> {
    match icon {
        "x-office-document" | "x-office-spreadsheet" | "x-office-presentation" => {
            Some(ContentCategory::Document)
        }
        "package-x-generic" | "media-optical" => Some(ContentCategory::Archive),
        "application-x-executable" | "universal-application-bundle" => {
            Some(ContentCategory::Executable)
        }
        "text-html" | "text-x-script" => {
            if linguist::is_data_format(mime, name) {
                Some(ContentCategory::Text)
            } else {
                Some(ContentCategory::Code)
            }
        }
        "text-x-generic" | "text-x-generic-template" => Some(ContentCategory::Text),
        "image-x-generic" => Some(ContentCategory::Image),
        "audio-x-generic" => Some(ContentCategory::Audio),
        "video-x-generic" => Some(ContentCategory::Video),
        "font-x-generic" => Some(ContentCategory::Font),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
