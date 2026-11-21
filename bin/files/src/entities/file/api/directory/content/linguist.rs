//! Generated GitHub Linguist source-code and structured-data lookups.
//!
//! Supplies zero-I/O fallback for modern developer extensions absent from
//! `shared-mime-info`, refines `text/*` MIME matches into `Code` vs `Text`, and
//! provides candidate metadata to the unified [`super::disambiguation`] module.

mod generated;

use std::ffi::OsStr;

use generated::{
    CODE_EXTENSIONS, CODE_FILENAMES, CODE_MIMES, DATA_EXTENSIONS, DATA_FILENAMES, DATA_MIMES,
};

/// Text/code format from GitHub Linguist used during structural disambiguation
/// when `shared-mime-info` binary/XML candidates fail validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LinguistFallback {
    Code(&'static str),
    Data(&'static str),
}

/// Candidate metadata for checking whether a single binary/XML `shared-mime-info`
/// match is shadowed by a Linguist text/code language (Pattern 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShadowCandidate {
    pub(super) fallback: LinguistFallback,
    /// `true` for XML/HTML-based languages (e.g. XHTML, XSLT) that must NOT treat
    /// an XML `shared-mime-info` match as a shadow conflict.
    pub(super) xml_based: bool,
    /// `true` when matched via an exact literal filename (e.g. `go.mod`, `Dockerfile`)
    /// rather than a suffix.
    pub(super) exact_filename: bool,
}

/// Look up an unambiguous source-code language name for zero-I/O C0 fallback
/// when `shared-mime-info` has no filename match.
pub(super) fn code_language_for_filename(name: &OsStr) -> Option<&'static str> {
    let text = name.to_str()?;
    if let Ok(idx) = CODE_FILENAMES.binary_search_by_key(&text, |(entry, _)| *entry) {
        return CODE_FILENAMES[idx].1;
    }
    let lower = text.to_ascii_lowercase();
    for suffix in candidate_extensions(&lower).into_iter().flatten() {
        if let Ok(idx) = CODE_EXTENSIONS.binary_search_by_key(&suffix, |(entry, _, _)| *entry)
            && let Some(language) = CODE_EXTENSIONS[idx].1
        {
            return Some(language);
        }
    }
    None
}

/// Look up a Linguist candidate that may shadow a legacy binary or XML MIME rule
/// (e.g. `.tsx` -> TSX, `.mts` -> TypeScript, `.mdx` -> MDX, `go.mod` -> Go Module).
pub(super) fn shadowing_candidate(name: &OsStr) -> Option<ShadowCandidate> {
    let text = name.to_str()?;
    if let Ok(idx) = CODE_FILENAMES.binary_search_by_key(&text, |(entry, _)| *entry)
        && let Some(language) = CODE_FILENAMES[idx].1
    {
        return Some(ShadowCandidate {
            fallback: LinguistFallback::Code(language),
            xml_based: false,
            exact_filename: true,
        });
    }
    if let Ok(idx) = DATA_FILENAMES.binary_search_by_key(&text, |(entry, _)| *entry) {
        return Some(ShadowCandidate {
            fallback: LinguistFallback::Data(DATA_FILENAMES[idx].1),
            xml_based: false,
            exact_filename: true,
        });
    }
    let lower = text.to_ascii_lowercase();
    for suffix in candidate_extensions(&lower).into_iter().flatten() {
        if let Ok(idx) = CODE_EXTENSIONS.binary_search_by_key(&suffix, |(entry, _, _)| *entry) {
            let (_, language, xml_based) = CODE_EXTENSIONS[idx];
            if let Some(language) = language {
                return Some(ShadowCandidate {
                    fallback: LinguistFallback::Code(language),
                    xml_based,
                    exact_filename: false,
                });
            }
        }
    }
    None
}

/// Look up a Linguist fallback language when all `shared-mime-info` candidates
/// fail structural validation on plain-text content.
pub(super) fn disambiguation_fallback(name: &OsStr) -> Option<LinguistFallback> {
    shadowing_candidate(name).map(|candidate| candidate.fallback)
}

/// Whether a `text/*` MIME (and optional filename) represents source code or
/// code markup rather than prose/data text.
pub(super) fn is_code_text(mime: &str, name: Option<&OsStr>) -> bool {
    if mime == "text/plain" || DATA_MIMES.binary_search(&mime).is_ok() {
        return false;
    }
    if CODE_MIMES.binary_search(&mime).is_ok() {
        return true;
    }
    let Some(text) = name.and_then(OsStr::to_str) else {
        return false;
    };
    if CODE_FILENAMES
        .binary_search_by_key(&text, |(entry, _)| *entry)
        .is_ok()
    {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    for suffix in candidate_extensions(&lower).into_iter().flatten() {
        if CODE_EXTENSIONS
            .binary_search_by_key(&suffix, |(entry, _, _)| *entry)
            .is_ok()
        {
            return true;
        }
    }
    false
}

/// Whether a MIME type (and optional filename) represents structured data
/// (JSON, XML, YAML, TOML, etc.) that should stay in `Text` rather than `Code`
/// even when XDG assigns a `text-x-script` or `text-html` generic icon.
pub(super) fn is_data_format(mime: &str, name: Option<&OsStr>) -> bool {
    if DATA_MIMES.binary_search(&mime).is_ok() {
        return true;
    }
    let Some(text) = name.and_then(OsStr::to_str) else {
        return false;
    };
    let lower = text.to_ascii_lowercase();
    for suffix in candidate_extensions(&lower).into_iter().flatten() {
        if DATA_EXTENSIONS.binary_search(&suffix).is_ok()
            && CODE_EXTENSIONS
                .binary_search_by_key(&suffix, |(entry, _, _)| *entry)
                .is_err()
        {
            return true;
        }
    }
    false
}

/// Yield compound two-segment and final single-segment extensions (without leading dot)
/// for a lowercased filename, ignoring leading dotfiles without an extension.
fn candidate_extensions(lower_name: &str) -> [Option<&str>; 2] {
    let trimmed = lower_name.trim_start_matches('.');
    let Some(last_dot) = trimmed.rfind('.') else {
        return [None, None];
    };
    let single = &trimmed[last_dot + 1..];
    if single.is_empty() {
        return [None, None];
    }
    let compound = trimmed[..last_dot]
        .rfind('.')
        .map(|prev_dot| &trimmed[prev_dot + 1..])
        .filter(|ext| !ext.is_empty());
    [compound, Some(single)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_unambiguous_code_languages_and_keeps_data_out_of_code() {
        assert_eq!(
            code_language_for_filename(OsStr::new("main.zig")),
            Some("Zig")
        );
        assert_eq!(
            code_language_for_filename(OsStr::new("flake.nix")),
            Some("Nix")
        );
        assert_eq!(
            code_language_for_filename(OsStr::new("Dockerfile")),
            Some("Dockerfile")
        );
        assert_eq!(
            code_language_for_filename(OsStr::new("justfile")),
            Some("Just")
        );
        assert_eq!(code_language_for_filename(OsStr::new("notes.txt")), None);
        assert_eq!(code_language_for_filename(OsStr::new("README.md")), None);
        assert_eq!(code_language_for_filename(OsStr::new("config.json")), None);
        assert_eq!(code_language_for_filename(OsStr::new("data.bin")), None);

        assert_eq!(
            disambiguation_fallback(OsStr::new("index.ts")),
            Some(LinguistFallback::Code("TypeScript"))
        );
        assert_eq!(
            disambiguation_fallback(OsStr::new("App.tsx")),
            Some(LinguistFallback::Code("TSX"))
        );
        assert_eq!(
            disambiguation_fallback(OsStr::new("go.mod")),
            Some(LinguistFallback::Data("Go Module"))
        );

        assert!(is_code_text("text/rust", Some(OsStr::new("lib.rs"))));
        assert!(is_code_text("text/x-python", None));
        assert!(is_code_text("text/html", Some(OsStr::new("index.html"))));
        assert!(is_code_text("text/css", Some(OsStr::new("app.css"))));
        assert!(!is_code_text("text/plain", Some(OsStr::new("readme.txt"))));
        assert!(!is_code_text("text/csv", Some(OsStr::new("table.csv"))));

        assert!(is_data_format(
            "application/json",
            Some(OsStr::new("a.json"))
        ));
        assert!(is_data_format("application/xml", Some(OsStr::new("a.xml"))));
        assert!(!is_data_format("text/javascript", Some(OsStr::new("a.js"))));
    }
}
