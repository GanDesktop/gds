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
    borrow::Cow,
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
static ZEROSIZE_MIME: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from("application/x-zerosize"));

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

/// Full content detection for a single regular file (used by `Entry::inspect`
/// and Properties). Preloads the localized XML description off the UI thread.
pub(super) fn detect(name: &OsStr, path: &Path) -> Option<ContentType> {
    let detected = detect_with_database(&MIME_DB, name, path);
    if let Some(mime) = detected.as_ref().and_then(|c| c.mime.as_deref()) {
        descriptions::preload_mime(mime);
    }
    detected
}

/// Zero-I/O filename classification distinguishing resolved, ambiguous, and unmatched entries.
#[cfg(test)]
pub(super) fn classify_filename(name: &OsStr) -> FilenameClassification {
    classify_filename_with_database(&MIME_DB, name)
}

/// Base classification for a local regular file: checks an explicit `user.mime_type`
/// extended attribute before falling back to zero-I/O filename matching.
///
/// Sets `*xattr_supported = false` if the underlying filesystem reports that extended
/// attributes are unsupported so subsequent entries in the same directory skip the syscall.
pub(super) fn classify_file_base(
    name: &OsStr,
    path: &Path,
    xattr_supported: &mut bool,
) -> FilenameClassification {
    if *xattr_supported {
        match gds::service::file_tags::lookup_mime_type(path) {
            gds::service::file_tags::XattrLookup::Present(xattr_mime) => {
                let canonical = MIME_DB.unalias(&xattr_mime);
                return FilenameClassification::Resolved(build_from_mime(
                    &MIME_DB,
                    &canonical,
                    Some(name),
                    DetectionSource::Xattr,
                ));
            }
            gds::service::file_tags::XattrLookup::Unsupported => {
                *xattr_supported = false;
            }
            gds::service::file_tags::XattrLookup::Absent => {}
        }
    }
    classify_filename_with_database(&MIME_DB, name)
}

/// Zero-I/O content classification for callers that never perform content probing
/// (e.g. remote directories or trash listings).
pub(crate) fn detect_from_name(name: &OsStr) -> Option<ContentType> {
    classify_filename_with_database(&MIME_DB, name).into_content()
}

/// Breadth-first MIME superclasses from `shared-mime-info` for external thumbnailer matching.
pub(super) fn mime_ancestors(mime: &str) -> Vec<Arc<str>> {
    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    let mut ancestors = Vec::new();
    let canonical = MIME_DB.unalias(mime).into_owned();
    visited.insert(canonical.clone());
    queue.push_back(canonical);
    while let Some(current) = queue.pop_front() {
        for parent in MIME_DB.direct_parents(&current) {
            if visited.insert(parent.clone()) {
                ancestors.push(Arc::from(parent.as_str()));
                if ancestors.len() >= 8 {
                    return ancestors;
                }
                queue.push_back(parent);
            }
        }
    }
    ancestors
}

/// Bounded 8 KiB content probe for ambiguous or unmatched regular files during
/// background enrichment. Also preloads the resolved MIME's XML description.
pub(super) fn probe_content(name: &OsStr, path: &Path) -> Option<ContentType> {
    let detected = probe_content_with_database(&MIME_DB, name, path);
    if let Some(mime) = detected.as_ref().and_then(|c| c.mime.as_deref()) {
        descriptions::preload_mime(mime);
    }
    detected
}

fn detect_with_database(db: &MimeDatabase, name: &OsStr, path: &Path) -> Option<ContentType> {
    if let Some(xattr_mime) = gds::service::file_tags::read_mime_type(path) {
        let canonical = db.unalias(&xattr_mime);
        return Some(build_from_mime(
            db,
            &canonical,
            Some(name),
            DetectionSource::Xattr,
        ));
    }
    match classify_filename_with_database(db, name) {
        FilenameClassification::Resolved(content) => Some(content),
        FilenameClassification::Ambiguous(fallback) => {
            probe_content_with_database(db, name, path).or(Some(fallback))
        }
        FilenameClassification::Unmatched => probe_content_with_database(db, name, path),
    }
}

fn classify_filename_with_database(db: &MimeDatabase, name: &OsStr) -> FilenameClassification {
    let candidates = db.filename_candidates(&name.to_string_lossy());
    if candidates.is_empty() {
        // Branch C0: zero-I/O Linguist code language fallback (e.g. `.zig`, `.nix`, `Dockerfile`).
        return if let Some(language) = linguist::code_language_for_filename(name) {
            FilenameClassification::Resolved(ContentType::from_linguist(language))
        } else {
            FilenameClassification::Unmatched
        };
    }

    match disambiguation::detect_conflict(db, name, &candidates) {
        ConflictKind::None => {
            // Branch A (unique non-shadowed MIME) or B0 (parent + subclasses collapsed to base).
            let resolved = disambiguation::converge_candidates(db, &candidates);
            FilenameClassification::Resolved(build_from_mime(
                db,
                &resolved,
                Some(name),
                DetectionSource::Filename,
            ))
        }
        ConflictKind::MultiMime | ConflictKind::LinguistShadow => {
            // Branch B1: multi-MIME or binary/XML shadow conflict pending 8 KiB disambiguation.
            let fallback = disambiguation::converge_candidates(db, &candidates);
            FilenameClassification::Ambiguous(build_from_mime(
                db,
                &fallback,
                Some(name),
                DetectionSource::Filename,
            ))
        }
    }
}

fn probe_content_with_database(
    db: &MimeDatabase,
    name: &OsStr,
    path: &Path,
) -> Option<ContentType> {
    if let Some(xattr_mime) = gds::service::file_tags::read_mime_type(path) {
        let canonical = db.unalias(&xattr_mime);
        return Some(build_from_mime(
            db,
            &canonical,
            Some(name),
            DetectionSource::Xattr,
        ));
    }
    let candidates = db.filename_candidates(&name.to_string_lossy());
    if !candidates.is_empty()
        && disambiguation::detect_conflict(db, name, &candidates) != ConflictKind::None
    {
        let header = read_header(path);
        let outcome = disambiguation::resolve_with_header(db, name, &candidates, header.as_deref());
        return Some(match outcome {
            Outcome::Mime { mime, source } => build_from_mime(db, &mime, Some(name), source),
            Outcome::Linguist(LinguistFallback::Code(language)) => {
                ContentType::from_linguist(language)
            }
            Outcome::Linguist(LinguistFallback::Data(language)) => {
                ContentType::from_linguist_data(language)
            }
        });
    }

    // Branch C: unmatched filename — read up to 8 KiB.
    let header = read_header(path)?;
    if header.is_empty() {
        return Some(build_from_mime(
            db,
            "application/x-zerosize",
            Some(name),
            DetectionSource::Heuristic,
        ));
    }
    if let Some((raw_mime, _)) = db.sniff_data(&header) {
        let mime = disambiguation::sanitize_disguised_desktop(name, raw_mime);
        let mime = if is_generic_xml_mime(&mime) {
            refine_xml_mime(db, &header).unwrap_or(mime)
        } else {
            mime
        };
        return Some(build_from_mime(
            db,
            &mime,
            Some(name),
            DetectionSource::Magic,
        ));
    }
    if disambiguation::looks_like_text(&header)
        && let Some(refined) = refine_xml_mime(db, &header)
    {
        return Some(build_from_mime(
            db,
            &refined,
            Some(name),
            DetectionSource::Magic,
        ));
    }
    Some(text_or_binary(&header))
}

fn build_from_mime(
    db: &MimeDatabase,
    mime: &str,
    name: Option<&OsStr>,
    source: DetectionSource,
) -> ContentType {
    let category = category_for_mime(db, mime, name);
    let text_previewable = matches!(category, ContentCategory::Text | ContentCategory::Code)
        || (category == ContentCategory::Document && db.is_subclass(mime, "text/plain"));
    let interned = match mime {
        "text/plain" => TEXT_PLAIN_MIME.clone(),
        "application/octet-stream" => OCTET_STREAM_MIME.clone(),
        "application/x-zerosize" => ZEROSIZE_MIME.clone(),
        other => Arc::from(other),
    };
    ContentType::from_mime(category, interned, source, text_previewable)
}

/// Stage 2 decision tree: derive `ContentCategory` from `shared-mime-info`
/// (`icons`, `generic-icons`, MIME prefix, `subclasses`), Linguist `Code` refinement,
/// and the minimal last-resort patch table.
fn category_for_mime(db: &MimeDatabase, mime: &str, name: Option<&OsStr>) -> ContentCategory {
    // 1. Own explicit icon / generic-icon hint in `shared-mime-info`.
    if let Some(category) = db
        .explicit_icon(mime)
        .and_then(|icon| map_generic_icon(&icon, mime, name))
        .or_else(|| {
            db.explicit_generic_icon(mime)
                .and_then(|icon| map_generic_icon(&icon, mime, name))
        })
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

fn text_or_binary(header: &[u8]) -> ContentType {
    if header.is_empty() || disambiguation::looks_like_text(header) {
        ContentType::from_mime(
            ContentCategory::Text,
            TEXT_PLAIN_MIME.clone(),
            DetectionSource::Heuristic,
            true,
        )
    } else {
        ContentType::from_mime(
            ContentCategory::Binary,
            OCTET_STREAM_MIME.clone(),
            DetectionSource::Heuristic,
            false,
        )
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct XmlRootElement<'a> {
    namespace_uri: Cow<'a, str>,
    local_name: &'a str,
}

fn is_generic_xml_mime(mime: &str) -> bool {
    matches!(mime, "application/xml" | "text/xml")
}

fn refine_xml_mime(db: &MimeDatabase, header: &[u8]) -> Option<String> {
    let root = extract_xml_root_element(header)?;
    db.match_xml_namespace(root.namespace_uri.as_ref(), root.local_name)
}

/// Extract the root XML opening tag's `(namespace_uri, local_name)` from a
/// bounded (possibly truncated) header buffer without requiring a complete
/// XML document.
fn extract_xml_root_element(bytes: &[u8]) -> Option<XmlRootElement<'_>> {
    let text = match std::str::from_utf8(bytes) {
        Ok(valid) => valid,
        Err(error) => {
            // Allow a valid multi-byte UTF-8 sequence cut off by the 8 KiB read limit.
            if error.error_len().is_some() {
                return None;
            }
            std::str::from_utf8(&bytes[..error.valid_up_to()]).ok()?
        }
    };
    let mut rest = text.strip_prefix('\u{FEFF}').unwrap_or(text);

    loop {
        rest = trim_xml_leading_whitespace(rest);
        if let Some(after_pi) = rest.strip_prefix("<?") {
            let end = after_pi.find("?>")?;
            rest = &after_pi[end + 2..];
            continue;
        }
        if let Some(after_comment) = rest.strip_prefix("<!--") {
            let end = after_comment.find("-->")?;
            rest = &after_comment[end + 3..];
            continue;
        }
        if let Some(after_bang) = rest.strip_prefix("<!") {
            let after_kw = after_bang
                .as_bytes()
                .get(..7)
                .filter(|kw| kw.eq_ignore_ascii_case(b"doctype"))
                .and_then(|_| after_bang.get(7..))?;
            if !after_kw.chars().next().is_some_and(is_xml_whitespace) {
                return None;
            }
            rest = skip_xml_doctype(after_kw)?;
            continue;
        }
        break;
    }

    let after_lt = rest.strip_prefix('<')?;
    let first = after_lt.chars().next()?;
    if !is_xml_name_start(first) {
        return None;
    }
    let name_len: usize = after_lt
        .chars()
        .take_while(|&ch| is_xml_name_char(ch))
        .map(char::len_utf8)
        .sum();
    let tag_name = &after_lt[..name_len];
    let (prefix, local_name) = match tag_name.split_once(':') {
        Some((p, local)) if !p.is_empty() && !local.is_empty() && !local.contains(':') => {
            (Some(p), local)
        }
        Some(_) => return None,
        None => (None, tag_name),
    };

    let mut tail = &after_lt[name_len..];
    let mut found_ns: Option<Cow<'_, str>> = None;

    loop {
        let trimmed = trim_xml_leading_whitespace(tail);
        let saw_whitespace = trimmed.len() < tail.len();
        tail = trimmed;

        if tail.starts_with('>') || tail.starts_with("/>") {
            return Some(XmlRootElement {
                namespace_uri: found_ns.unwrap_or(Cow::Borrowed("")),
                local_name,
            });
        }
        if tail.is_empty() {
            return found_ns.map(|namespace_uri| XmlRootElement {
                namespace_uri,
                local_name,
            });
        }
        if !saw_whitespace {
            return None;
        }

        let attr_first = tail.chars().next()?;
        if !is_xml_name_start(attr_first) {
            return None;
        }
        let attr_len: usize = tail
            .chars()
            .take_while(|&ch| is_xml_name_char(ch))
            .map(char::len_utf8)
            .sum();
        let attr_name = &tail[..attr_len];
        tail = trim_xml_leading_whitespace(&tail[attr_len..]);
        let after_eq = tail.strip_prefix('=')?;
        tail = trim_xml_leading_whitespace(after_eq);
        let (quote, after_quote) = if let Some(after) = tail.strip_prefix('"') {
            ('"', after)
        } else {
            ('\'', tail.strip_prefix('\'')?)
        };
        let Some(val_end) = after_quote.find(quote) else {
            return found_ns.map(|namespace_uri| XmlRootElement {
                namespace_uri,
                local_name,
            });
        };
        let raw_value = &after_quote[..val_end];
        tail = &after_quote[val_end + 1..];

        let matches_target_ns = match prefix {
            None => attr_name == "xmlns",
            Some(expected) => attr_name.strip_prefix("xmlns:") == Some(expected),
        };
        if matches_target_ns && found_ns.is_none() {
            found_ns = Some(decode_xml_attr_value(raw_value));
        }
    }
}

fn skip_xml_doctype(after_doctype: &str) -> Option<&str> {
    let bytes = after_doctype.as_bytes();
    let mut idx = 0usize;
    let mut bracket_depth = 0usize;
    let mut quote: Option<u8> = None;

    while idx < bytes.len() {
        let byte = bytes[idx];
        if let Some(active_quote) = quote {
            if byte == active_quote {
                quote = None;
            }
            idx += 1;
            continue;
        }
        if bracket_depth > 0
            && let Some(after_comment) = bytes[idx..].strip_prefix(b"<!--")
        {
            let end = after_comment
                .windows(3)
                .position(|window| window == b"-->")?;
            idx += 4 + end + 3;
            continue;
        }
        match byte {
            b'"' | b'\'' => quote = Some(byte),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'>' if bracket_depth == 0 => return Some(&after_doctype[idx + 1..]),
            _ => {}
        }
        idx += 1;
    }
    None
}

fn decode_xml_attr_value(raw: &str) -> Cow<'_, str> {
    if !raw.contains('&') {
        return Cow::Borrowed(raw);
    }
    Cow::Owned(
        raw.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
}

fn is_xml_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\r' | '\n')
}

fn trim_xml_leading_whitespace(input: &str) -> &str {
    input.trim_start_matches(is_xml_whitespace)
}

fn is_xml_name_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

fn is_xml_name_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':')
}

#[cfg(test)]
mod tests;
