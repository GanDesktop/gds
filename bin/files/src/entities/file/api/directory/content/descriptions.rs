//! Localized MIME `<comment>` lookup and in-memory caching from XDG per-type XML files.
//!
//! XML files (`<mime_dir>/<type>/<subtype>.xml`) are read exclusively off the UI
//! thread via [`preload_mimes`] or [`preload_mime`]; UI code queries the in-memory
//! cache through [`cached_comments`] without performing disk I/O.
//!
//! Language selection uses `unic_langid::LanguageIdentifier` with CLDR
//! `likelysubtags` (`maximize`) so script and region matching follow BCP 47 /
//! UTS #35 uniformly for every language without per-language special cases.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, RwLock},
};

use gds::i18n::Language;
use unic_langid::LanguageIdentifier;

use super::MIME_DB;

const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// Parsed `<comment>` entries for one canonical MIME type across all languages.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MimeComments {
    /// Untagged `<comment>` (neutral English per the freedesktop specification).
    default: Option<Arc<str>>,
    /// Tagged `<comment xml:lang="...">` entries in XDG precedence order.
    entries: Vec<(LanguageIdentifier, Arc<str>)>,
}

impl MimeComments {
    /// Localized `<comment>` matching the active GDS interface language (if available).
    pub(crate) fn for_language(&self, language: Language) -> Option<&str> {
        self.for_locale(language.locale())
    }

    /// Localized `<comment>` matching an arbitrary BCP 47 locale tag (e.g. `en-US`, `zh-CN`, `ja-JP`).
    pub(crate) fn for_locale(&self, locale: &str) -> Option<&str> {
        let requested: LanguageIdentifier = locale.parse().ok()?;
        let mut requested_max = requested.clone();
        requested_max.maximize();

        let mut best: Option<(u8, &str)> = None;

        // Per shared-mime-info spec, the untagged `<comment>` is neutral English (`en`).
        if requested.language.as_str() == "en"
            && let Some(default) = self.default.as_deref()
        {
            best = Some((1, default));
        }

        for (tag, text) in &self.entries {
            let Some(rank) = match_rank(&requested, &requested_max, tag) else {
                continue;
            };
            if best.is_none_or(|(best_rank, _)| rank < best_rank) {
                best = Some((rank, text.as_ref()));
            }
        }
        best.map(|(_, text)| text)
    }

    /// Default English `<comment>` fallback from the database.
    pub(crate) fn default_english(&self) -> Option<&str> {
        self.default.as_deref().or_else(|| self.for_locale("en"))
    }

    /// Merge lower-precedence XDG directory comments without overwriting existing entries.
    fn merge_lower_precedence(&mut self, lower: Self) {
        if self.default.is_none() {
            self.default = lower.default;
        }
        for (tag, text) in lower.entries {
            if !self.entries.iter().any(|(existing, _)| *existing == tag) {
                self.entries.push((tag, text));
            }
        }
    }
}

/// Rank how well `candidate` satisfies `requested` using CLDR likely-subtag
/// maximization (lower is better; `None` means incompatible language or script).
fn match_rank(
    requested: &LanguageIdentifier,
    requested_max: &LanguageIdentifier,
    candidate: &LanguageIdentifier,
) -> Option<u8> {
    let mut candidate_max = candidate.clone();
    candidate_max.maximize();

    if requested_max.language != candidate_max.language {
        return None;
    }
    if requested_max.script.is_some()
        && candidate_max.script.is_some()
        && requested_max.script != candidate_max.script
    {
        return None;
    }
    if requested.region.is_some() && requested.region == candidate.region {
        Some(0)
    } else if candidate.region.is_none() {
        Some(1)
    } else {
        Some(2)
    }
}

/// Thread-safe per-type XML comment store backed by a fixed list of XDG `mime/` directories.
pub(crate) struct DescriptionStore {
    mime_dirs: Vec<PathBuf>,
    cache: RwLock<HashMap<Arc<str>, MimeComments>>,
}

impl DescriptionStore {
    pub(crate) fn new(mime_dirs: Vec<PathBuf>) -> Self {
        Self {
            mime_dirs,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Pure in-memory lookup; never touches the filesystem.
    pub(crate) fn cached(&self, canonical_mime: &str) -> Option<MimeComments> {
        self.cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(canonical_mime)
            .cloned()
    }

    /// Whether `canonical_mime` has already been loaded into the cache.
    pub(crate) fn is_cached(&self, canonical_mime: &str) -> bool {
        self.cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(canonical_mime)
    }

    /// Load `<comment>` entries for `canonical_mime` off the UI thread and cache them.
    /// Returns `true` if the cache was updated with a newly loaded entry.
    pub(crate) fn preload_one(&self, canonical_mime: &str) -> bool {
        if self.is_cached(canonical_mime) {
            return false;
        }
        let parsed = self.read_from_disk(canonical_mime);
        let mut guard = self
            .cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.contains_key(canonical_mime) {
            return false;
        }
        guard.insert(Arc::from(canonical_mime), parsed);
        true
    }

    /// Load `<comment>` entries for any uncached MIMEs in `mimes` off the UI thread.
    /// Returns `true` if at least one new entry was inserted into the cache.
    pub(crate) fn preload_many<I, S>(&self, mimes: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut loaded_any = false;
        for mime in mimes {
            if self.preload_one(mime.as_ref()) {
                loaded_any = true;
            }
        }
        loaded_any
    }

    fn read_from_disk(&self, canonical_mime: &str) -> MimeComments {
        let Some((media_type, subtype)) = split_safe_mime(canonical_mime) else {
            return MimeComments::default();
        };

        let mut merged = MimeComments::default();
        for dir in &self.mime_dirs {
            let xml_path = dir.join(media_type).join(format!("{subtype}.xml"));
            let Ok(xml_text) = fs::read_to_string(&xml_path) else {
                continue;
            };
            merged.merge_lower_precedence(parse_xml_comments(&xml_text));
        }
        merged
    }
}

static STORE: LazyLock<DescriptionStore> =
    LazyLock::new(|| DescriptionStore::new(MIME_DB.mime_dirs().to_vec()));

/// Look up cached localized `<comment>` entries without performing disk I/O.
pub(crate) fn cached_comments(canonical_mime: &str) -> Option<MimeComments> {
    STORE.cached(canonical_mime)
}

/// Whether `canonical_mime` is already present in the in-memory description cache.
pub(crate) fn is_cached(canonical_mime: &str) -> bool {
    STORE.is_cached(canonical_mime)
}

/// Load and cache per-type XML `<comment>` entries for one canonical MIME off the UI thread.
pub(crate) fn preload_mime(canonical_mime: &str) -> bool {
    STORE.preload_one(canonical_mime)
}

/// Load and cache per-type XML `<comment>` entries for a batch of canonical MIMEs off the UI thread.
pub(crate) fn preload_mimes<I, S>(mimes: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    STORE.preload_many(mimes)
}

/// Parse `<comment>` entries from a single `shared-mime-info` per-type XML document.
pub(crate) fn parse_xml_comments(xml: &str) -> MimeComments {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return MimeComments::default();
    };

    let mut parsed = MimeComments::default();
    for node in doc
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "comment")
    {
        let Some(raw_text) = node.text() else {
            continue;
        };
        let text = raw_text.trim();
        if text.is_empty() {
            continue;
        }
        let lang = node
            .attribute((XML_NAMESPACE, "lang"))
            .or_else(|| node.attribute("lang"));
        match lang {
            None => {
                if parsed.default.is_none() {
                    parsed.default = Some(Arc::from(text));
                }
            }
            Some(raw_lang) => {
                if let Ok(tag) = raw_lang.trim().parse::<LanguageIdentifier>()
                    && !parsed.entries.iter().any(|(existing, _)| *existing == tag)
                {
                    parsed.entries.push((tag, Arc::from(text)));
                }
            }
        }
    }
    parsed
}

fn split_safe_mime(mime: &str) -> Option<(&str, &str)> {
    let (media_type, subtype) = mime.split_once('/')?;
    if is_safe_segment(media_type) && is_safe_segment(subtype) {
        Some((media_type, subtype))
    } else {
        None
    }
}

fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.contains(['/', '\\', '\0'])
        && Path::new(segment).components().count() == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcp47_matching_respects_script_and_region_across_languages() {
        let epub_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<mime-type xmlns="http://www.freedesktop.org/standards/shared-mime-info" type="application/epub+zip">
  <comment>Electronic book document</comment>
  <comment xml:lang="zh-Hant-TW">電子書文件</comment>
  <comment xml:lang="en-GB">Electronic book (UK)</comment>
  <comment xml:lang="ja">電子書籍</comment>
  <comment xml:lang="sr-Cyrl">Електронска књига</comment>
</mime-type>"#;
        let comments = parse_xml_comments(epub_xml);
        assert_eq!(comments.for_locale("zh-CN"), None);
        assert_eq!(comments.for_locale("zh-TW"), Some("電子書文件"));
        assert_eq!(
            comments.for_locale("en-US"),
            Some("Electronic book document")
        );
        assert_eq!(comments.for_locale("en-GB"), Some("Electronic book (UK)"));
        assert_eq!(comments.for_locale("ja-JP"), Some("電子書籍"));
        assert_eq!(comments.for_locale("sr-RS"), Some("Електронска књига"));
        assert_eq!(comments.for_locale("sr-Latn-RS"), None);
        assert_eq!(comments.default_english(), Some("Electronic book document"));

        let blender_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<mime-type xmlns="http://www.freedesktop.org/standards/shared-mime-info" type="application/x-blender">
  <comment>Blender scene</comment>
  <comment xml:lang="zh-Hant-TW">Blender 場景</comment>
  <comment xml:lang="zh-Hans-CN">Blender 场景</comment>
</mime-type>"#;
        let comments = parse_xml_comments(blender_xml);
        assert_eq!(
            comments.for_language(Language::Chinese),
            Some("Blender 场景")
        );
        assert_eq!(
            comments.for_language(Language::English),
            Some("Blender scene")
        );

        let underscore_xml = r#"<mime-type type="application/x-demo">
  <comment>Demo file</comment>
  <comment xml:lang="zh_CN">示例文件</comment>
</mime-type>"#;
        let comments = parse_xml_comments(underscore_xml);
        assert_eq!(comments.for_language(Language::Chinese), Some("示例文件"));

        // Unscripted Traditional Chinese candidate (`zh-TW`) must not match `zh-CN`.
        let unscripted_tw_xml = r#"<mime-type type="application/x-tw-only">
  <comment>TW only</comment>
  <comment xml:lang="zh-TW">僅繁體</comment>
</mime-type>"#;
        let comments = parse_xml_comments(unscripted_tw_xml);
        assert_eq!(comments.for_locale("zh-CN"), None);
        assert_eq!(comments.for_locale("zh-Hant-TW"), Some("僅繁體"));
    }

    #[test]
    fn store_respects_xdg_directory_precedence_and_fills_missing_slots() {
        let user = tempfile::tempdir().expect("user XDG dir");
        let sys = tempfile::tempdir().expect("system XDG dir");
        let user_app = user.path().join("application");
        let sys_app = sys.path().join("application");
        fs::create_dir_all(&user_app).expect("create user app dir");
        fs::create_dir_all(&sys_app).expect("create sys app dir");

        fs::write(
            user_app.join("x-blender.xml"),
            r#"<mime-type type="application/x-blender"><comment>Custom Blender</comment></mime-type>"#,
        )
        .expect("write user override");
        fs::write(
            sys_app.join("x-blender.xml"),
            r#"<mime-type type="application/x-blender">
  <comment>Blender scene</comment>
  <comment xml:lang="zh-Hans-CN">Blender 场景</comment>
</mime-type>"#,
        )
        .expect("write system xml");

        let store =
            DescriptionStore::new(vec![user.path().to_path_buf(), sys.path().to_path_buf()]);
        assert_eq!(store.cached("application/x-blender"), None);
        assert!(store.preload_one("application/x-blender"));
        assert!(!store.preload_one("application/x-blender"));

        let loaded = store
            .cached("application/x-blender")
            .expect("cached after preload");
        assert_eq!(
            loaded.for_language(Language::English),
            Some("Custom Blender")
        );
        assert_eq!(loaded.for_language(Language::Chinese), Some("Blender 场景"));
    }
}
