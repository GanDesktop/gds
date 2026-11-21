//! Unified filename conflict detection and bounded 8 KiB structural disambiguation.
//!
//! Handles two general conflict patterns without ad-hoc extension special cases:
//!
//! 1. **Multi-MIME filename conflicts (Pattern 1):** `shared-mime-info` yields
//!    multiple unrelated canonical MIME candidates (e.g. `.ogg`, `.ts`, `.smi`,
//!    `.dot`, `.otf`). Disambiguation combines `magic` matching with structural
//!    validation (text vs binary AND `application/xml` subclass checks).
//! 2. **Legacy binary/XML shadow conflicts (Pattern 2):** `shared-mime-info`
//!    yields a single legacy binary or XML format (e.g. `.tsx` -> Tiled XML,
//!    `.mts` -> `video/mp2t`, `.mdx` -> Sega 32X ROM, `go.mod` -> `audio/x-mod`,
//!    `.sml` -> SMIL XML) that shadows a modern Linguist plain-text/code format.
//!
//! When content probing is disabled or the file is unreadable/empty, conflicts
//! fall back deterministically without disk I/O.

use std::ffi::OsStr;

use super::{
    DetectionSource,
    linguist::{self, LinguistFallback},
    mime_db::MimeDatabase,
};

/// High-confidence magic threshold from the XDG Shared MIME-info specification.
const HIGH_CONFIDENCE_MAGIC: u32 = 80;

/// Zero-I/O conflict classification for `shared-mime-info` filename candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConflictKind {
    /// No conflict: filename result resolves on the zero-I/O fast path.
    None,
    /// Multiple unrelated `shared-mime-info` candidates (e.g. `.ogg`, `.ts`, `.dot`).
    MultiMime,
    /// Single binary or XML `shared-mime-info` match shadowed by a Linguist text/code format
    /// (e.g. `.tsx`, `.mts`, `.mdx`, `go.mod`, `.sml`).
    LinguistShadow,
}

/// Outcome of disambiguating filename candidates against an 8 KiB file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Outcome {
    Mime {
        mime: String,
        source: DetectionSource,
    },
    Linguist(LinguistFallback),
}

/// Inspect `candidates` (from `shared-mime-info`) and `name` purely in memory
/// to decide whether bounded 8 KiB content disambiguation is warranted.
pub(super) fn detect_conflict(
    db: &MimeDatabase,
    name: &OsStr,
    candidates: &[String],
) -> ConflictKind {
    match candidates.len() {
        0 => ConflictKind::None,
        1 => {
            if is_shadowed_by_linguist(db, name, &candidates[0]) {
                ConflictKind::LinguistShadow
            } else {
                ConflictKind::None
            }
        }
        _ => {
            if db.common_superclass(candidates).is_some() {
                ConflictKind::None
            } else {
                ConflictKind::MultiMime
            }
        }
    }
}

/// Disambiguate `candidates` using up to 8 KiB of file `header` bytes.
///
/// Handles both [`ConflictKind::MultiMime`] and [`ConflictKind::LinguistShadow`]
/// through one unified magic + structural validation pipeline:
/// 1. Empty/missing headers fall back deterministically to `candidates`.
/// 2. Magic signatures matching a candidate (or its subclass), or high-confidence
///    magic (`>= 80`), resolve immediately; container magic narrows candidates.
/// 3. Structural filtering validates binary vs text (`text/plain` subclass) AND
///    XML structure (`application/xml` subclass requires XML-like leading bytes).
/// 4. If all system candidates fail structural validation on plain-text bytes,
///    adopt a matching Linguist text/code language (e.g. TypeScript for non-XML
///    `.ts`, TSX for non-XML `.tsx`, MDX for plain-text `.mdx`, Go Module for
///    plain-text `go.mod`).
pub(super) fn resolve_with_header(
    db: &MimeDatabase,
    name: &OsStr,
    candidates: &[String],
    header: Option<&[u8]>,
) -> Outcome {
    let Some(header) = header.filter(|bytes| !bytes.is_empty()) else {
        return Outcome::Mime {
            mime: converge_candidates(db, candidates),
            source: DetectionSource::Filename,
        };
    };

    let mut narrowed: Vec<String> = candidates.to_vec();
    if let Some((raw_mime, priority)) = db.sniff_data(header) {
        let sniffed = sanitize_disguised_desktop(name, raw_mime);
        if candidates.iter().any(|cand| db.is_subclass(&sniffed, cand)) {
            return Outcome::Mime {
                mime: sniffed,
                source: DetectionSource::Magic,
            };
        }
        if priority >= HIGH_CONFIDENCE_MAGIC {
            return Outcome::Mime {
                mime: sniffed,
                source: DetectionSource::Magic,
            };
        }
        let sub_matches: Vec<String> = candidates
            .iter()
            .filter(|cand| db.is_subclass(cand, &sniffed))
            .cloned()
            .collect();
        if sub_matches.len() == 1 {
            return Outcome::Mime {
                mime: sub_matches[0].clone(),
                source: DetectionSource::Magic,
            };
        }
        if !sub_matches.is_empty() {
            narrowed = sub_matches;
        }
    }

    let is_text = looks_like_text(header);
    let is_xml = is_text && looks_like_xml(header);

    let structurally_valid: Vec<String> = narrowed
        .iter()
        .filter(|cand| candidate_matches_structure(db, cand, is_text, is_xml))
        .cloned()
        .collect();

    if structurally_valid.len() == 1 {
        return Outcome::Mime {
            mime: structurally_valid[0].clone(),
            source: DetectionSource::Filename,
        };
    }
    if structurally_valid.len() > 1 {
        return Outcome::Mime {
            mime: converge_candidates(db, &structurally_valid),
            source: DetectionSource::Filename,
        };
    }

    // All `shared-mime-info` candidates failed structural validation (for example,
    // non-XML TypeScript `.ts`, non-XML React `.tsx`, plain-text `.mts`/`.mdx`,
    // or plain-text `go.mod`): adopt Linguist's text/code fallback when available.
    if is_text && let Some(fallback) = linguist::disambiguation_fallback(name) {
        return Outcome::Linguist(fallback);
    }

    Outcome::Mime {
        mime: converge_candidates(db, &narrowed),
        source: DetectionSource::Filename,
    }
}

/// Deterministic fallback convergence when probing is disabled, unreadable, or
/// leaves multiple structurally valid candidates.
pub(super) fn converge_candidates(db: &MimeDatabase, candidates: &[String]) -> String {
    if let Some(base) = db.common_superclass(candidates) {
        return base;
    }
    if let Some(parent) = db.shared_parent(candidates) {
        return parent;
    }
    candidates
        .iter()
        .find(|mime| db.explicit_generic_icon(mime).is_some())
        .or_else(|| candidates.first())
        .cloned()
        .unwrap_or_else(|| "application/octet-stream".to_owned())
}

/// GIO/XDG security rule: never sniff `application/x-desktop` from file contents
/// when the filename does not end with `.desktop`.
pub(super) fn sanitize_disguised_desktop(name: &OsStr, mime: String) -> String {
    if mime == "application/x-desktop"
        && !name
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".desktop")
    {
        "text/plain".to_owned()
    } else {
        mime
    }
}

/// Heuristic deciding whether a bounded header sample looks like human-readable text.
pub(super) fn looks_like_text(bytes: &[u8]) -> bool {
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

/// Lightweight structural check for whether text bytes begin like an XML document
/// (`<?xml`, `<!DOCTYPE`, or a well-formed root element `<Name...>`), skipping
/// optional UTF-8 BOM, whitespace, and leading XML comments.
pub(super) fn looks_like_xml(bytes: &[u8]) -> bool {
    let mut rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    loop {
        rest = trim_ascii_leading_whitespace(rest);
        if let Some(after_comment) = rest.strip_prefix(b"<!--") {
            let Some(end) = find_subslice(after_comment, b"-->") else {
                return false;
            };
            rest = &after_comment[end + 3..];
            continue;
        }
        break;
    }

    if rest.starts_with(b"<?xml") || starts_with_ascii_case_insensitive(rest, b"<!doctype") {
        return true;
    }

    // Root element `<TagName...>` (reject JSX fragments `<>`, comparisons `< `, etc.).
    let Some(after_lt) = rest.strip_prefix(b"<") else {
        return false;
    };
    let Some(&first) = after_lt.first() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return false;
    }
    let name_len = after_lt
        .iter()
        .take_while(|&&b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
        .count();
    let Some(&after_name) = after_lt.get(name_len) else {
        return false;
    };
    if !matches!(after_name, b'>' | b'/' | b' ' | b'\t' | b'\r' | b'\n') {
        return false;
    }
    // Require a closing `>` within the sampled header so stray `<ident` expressions fail.
    after_lt[name_len..].contains(&b'>')
}

fn is_shadowed_by_linguist(db: &MimeDatabase, name: &OsStr, system_mime: &str) -> bool {
    let Some(candidate) = linguist::shadowing_candidate(name) else {
        return false;
    };
    let is_system_text = db.is_subclass(system_mime, "text/plain");
    let is_system_xml = db.is_subclass(system_mime, "application/xml");

    // Pattern 2a: Exact Linguist filename (e.g. `go.mod`) shadowed by a binary MIME suffix.
    if candidate.exact_filename {
        return !is_system_text;
    }

    // Pattern 2b: Single non-text MIME shadowed by a Linguist code language
    // (e.g. `.mts` -> `video/mp2t`, `.mdx` -> Sega ROM, `.res` -> Godot binary).
    if !is_system_text {
        // Do not treat MIMEs already marked as scripts (`text-x-script`) as binary shadows.
        return db.explicit_generic_icon(system_mime).as_deref() != Some("text-x-script");
    }

    // Pattern 2c: Single XML subclass MIME shadowed by a non-XML Linguist code language
    // (e.g. `.tsx` -> Tiled XML, `.sml` -> SMIL XML). Genuine XML code formats
    // (`.xhtml`, `.xslt`, `application/xml`) stay on the zero-I/O fast path.
    is_system_xml
        && !candidate.xml_based
        && !matches!(
            system_mime,
            "application/xml" | "text/xml" | "application/xhtml+xml" | "application/xslt+xml"
        )
}

fn candidate_matches_structure(
    db: &MimeDatabase,
    candidate: &str,
    is_text: bool,
    is_xml: bool,
) -> bool {
    let expects_text = db.is_subclass(candidate, "text/plain");
    if expects_text != is_text {
        return false;
    }
    if is_text && db.is_subclass(candidate, "application/xml") && !is_xml {
        return false;
    }
    true
}

fn trim_ascii_leading_whitespace(mut bytes: &[u8]) -> &[u8] {
    while let Some((&first, tail)) = bytes.split_first() {
        if first.is_ascii_whitespace() {
            bytes = tail;
        } else {
            break;
        }
    }
    bytes
}

fn starts_with_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .get(..needle.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(needle))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests;
