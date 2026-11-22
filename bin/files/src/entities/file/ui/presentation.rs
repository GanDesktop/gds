//! Formatted labels and size strings for file entries.

use std::{ffi::OsStr, path::Path};

use gds::{
    app::Gds,
    i18n::{self, Catalog, FluentArgs, Language, t},
};
use zgui::prelude::{Get, GetUntracked};

use crate::entities::file::directory::{
    ChildCount, ContentCategory, DetectionSource, Entry, EntryKind, LinkTarget, mime_descriptions,
};

/// Shown for a metadata value the platform did not provide.
pub(crate) const UNKNOWN_VALUE: &str = "—";

/// Formats the non-icon cells of a file row; the details view shares these.
pub(crate) struct EntryPresentation;

/// Owner cell contents; the current user's name is emphasized.
pub(crate) struct OwnerLabel {
    pub(crate) text: String,
    pub(crate) current: bool,
}

impl EntryPresentation {
    pub(crate) fn owner(entry: &Entry) -> OwnerLabel {
        let Some(owner) = entry.details.owner.as_ref() else {
            return OwnerLabel {
                text: UNKNOWN_VALUE.into(),
                current: false,
            };
        };
        let name = owner.label().into_owned();
        if !owner.is_current_user {
            return OwnerLabel {
                text: name,
                current: false,
            };
        }
        OwnerLabel {
            text: t!("owner-you", name),
            current: true,
        }
    }

    pub(crate) fn group(entry: &Entry) -> String {
        entry
            .details
            .group
            .as_ref()
            .map_or_else(|| UNKNOWN_VALUE.into(), |group| group.label().into_owned())
    }

    /// Localized broad type, or the link relationship for a symbolic link.
    pub(crate) fn type_label(entry: &Entry) -> String {
        if entry.kind == EntryKind::Directory {
            return i18n::text("kind-directory");
        }
        if entry.kind != EntryKind::SymbolicLink {
            return Self::kind_label(entry);
        }
        let Some(target) = entry.details.target.as_ref() else {
            return i18n::text("kind-link");
        };
        match target {
            LinkTarget::Broken => i18n::text("link-broken"),
            LinkTarget::Unreachable => i18n::text("link-unreachable"),
            LinkTarget::Resolved { kind, .. } => {
                let target_label = match kind {
                    EntryKind::Directory => i18n::text("kind-directory"),
                    EntryKind::File => entry.details.display_content().map_or_else(
                        || i18n::text("kind-file"),
                        |content| Self::category_label(content.category),
                    ),
                    EntryKind::Other => i18n::text("kind-other"),
                    EntryKind::Unknown => i18n::text("kind-unknown"),
                    EntryKind::SymbolicLink => i18n::text("kind-link"),
                };
                t!("type-link", target = target_label)
            }
        }
    }

    /// Localized human-readable detailed type for the Detailed Type column and Properties.
    ///
    /// Reads only from the in-memory XML comment cache (zero disk I/O) and tracks
    /// the active GDS interface language reactively.
    pub(crate) fn detailed_type(entry: &Entry) -> String {
        let catalog = Gds::current().i18n().catalog().get();
        Self::detailed_type_with_catalog(entry, &catalog, mime_descriptions::cached_comments)
    }

    /// Untracked detailed type key used by `DetailedType` sorting so sort order matches
    /// the displayed localized text without creating reactive subscriptions.
    pub(crate) fn detailed_type_sort_key(entry: &Entry) -> Option<String> {
        if entry.kind == EntryKind::Directory {
            return None;
        }
        if let Some(gds) = Gds::try_current() {
            let catalog = gds.i18n().catalog().get_untracked();
            let label = Self::detailed_type_with_catalog(
                entry,
                &catalog,
                mime_descriptions::cached_comments,
            );
            return (label != UNKNOWN_VALUE && !label.is_empty()).then_some(label);
        }
        let content = entry.details.display_content()?;
        if let Some(language) = content.code_language.as_deref() {
            return Some(format!("{language} source code"));
        }
        if let Some(mime) = content.mime.as_deref() {
            if let Some(comments) = mime_descriptions::cached_comments(mime)
                && let Some(desc) = comments
                    .for_language(Language::English)
                    .or_else(|| comments.default_english())
            {
                return Some(desc.to_owned());
            }
            return Some(mime.to_owned());
        }
        None
    }

    /// Raw canonical MIME type for Properties display.
    pub(crate) fn mime_type(entry: &Entry) -> Option<&str> {
        entry
            .details
            .display_content()
            .and_then(|content| content.mime.as_deref())
    }

    /// Stage 3 decision tree: resolve the localized human-readable type name
    /// against an explicit Fluent `catalog` and in-memory `comments` lookup.
    pub(crate) fn detailed_type_with_catalog(
        entry: &Entry,
        catalog: &Catalog,
        comments: impl Fn(&str) -> Option<mime_descriptions::MimeComments>,
    ) -> String {
        if entry.kind == EntryKind::Directory {
            return catalog.message("kind-directory", None);
        }
        let ext = (entry.kind == EntryKind::File)
            .then(|| uppercase_extension(&entry.name))
            .flatten();

        let Some(content) = entry.details.display_content() else {
            return match ext {
                Some(ext) => format_ext_file(catalog, &ext),
                None => UNKNOWN_VALUE.into(),
            };
        };

        // Branch 2: Linguist code or structured-data match without a system MIME.
        if content.source == DetectionSource::Linguist
            && content.mime.is_none()
            && let Some(language) = content.code_language.as_deref()
        {
            return if content.category == ContentCategory::Code {
                format_source_code(catalog, language)
            } else {
                language.to_owned()
            };
        }

        let Some(mime) = content.mime.as_deref() else {
            return Self::category_fallback_with_catalog(catalog, content.category, ext.as_deref());
        };

        // Heuristic generic fallback on an unknown extension retains the extension.
        if content.source == DetectionSource::Heuristic
            && matches!(mime, "text/plain" | "application/octet-stream")
            && let Some(ext) = ext.as_deref()
        {
            return Self::category_fallback_with_catalog(catalog, content.category, Some(ext));
        }

        let language = catalog.language();
        let parsed = comments(mime);

        // 1. Primary truth: shared-mime-info per-type XML `<comment>` in GDS UI language.
        if let Some(desc) = parsed.as_ref().and_then(|c| c.for_language(language)) {
            return desc.to_owned();
        }

        // 2. Small Fluent missing-translation fallback.
        if let Some(key) = fluent_mime_fallback_key(mime) {
            return catalog.message(key, None);
        }

        // 3. Default English `<comment>` from shared-mime-info XML.
        if let Some(desc) = parsed.as_ref().and_then(|c| c.default_english()) {
            return desc.to_owned();
        }

        // 4. Missing database description fallback: localized category (+ extension).
        Self::category_fallback_with_catalog(catalog, content.category, ext.as_deref())
    }

    fn category_fallback_with_catalog(
        catalog: &Catalog,
        category: ContentCategory,
        ext: Option<&str>,
    ) -> String {
        let category_label = catalog.message(category_message_id(category), None);
        match ext {
            Some(ext) => {
                let mut args = FluentArgs::new();
                args.set("ext", ext);
                args.set("category", category_label);
                catalog.message("detailed-type-extension-category", Some(&args))
            }
            None => category_label,
        }
    }

    fn kind_label(entry: &Entry) -> String {
        match entry.kind {
            EntryKind::Directory => i18n::text("kind-directory"),
            EntryKind::File => entry.details.display_content().map_or_else(
                || i18n::text("kind-file"),
                |content| Self::category_label(content.category),
            ),
            EntryKind::SymbolicLink => i18n::text("kind-link"),
            EntryKind::Other => i18n::text("kind-other"),
            EntryKind::Unknown => i18n::text("kind-unknown"),
        }
    }

    pub(crate) fn category_label(category: ContentCategory) -> String {
        i18n::text(category_message_id(category))
    }

    /// Size cell contents: bytes for a file, the direct child count for a
    /// directory. A directory whose children could not be counted stays unknown.
    pub(crate) fn size_label(entry: &Entry) -> String {
        if entry.kind == EntryKind::Directory {
            return match entry.details.children {
                Some(children) => Self::child_count_label(children),
                None => UNKNOWN_VALUE.into(),
            };
        }
        entry.size.map_or_else(|| UNKNOWN_VALUE.into(), format_size)
    }

    /// A count that stopped at the read limit is shown as a lower bound so the
    /// cell never claims a precision it does not have.
    fn child_count_label(children: ChildCount) -> String {
        let id = if children.truncated {
            "directory-item-count-at-least"
        } else {
            "directory-item-count"
        };
        i18n::count(id, children.count as usize)
    }

    pub(crate) fn format_size(bytes: u64) -> String {
        format_size(bytes)
    }
}

fn category_message_id(category: ContentCategory) -> &'static str {
    match category {
        ContentCategory::Text => "content-text",
        ContentCategory::Code => "content-code",
        ContentCategory::Document => "content-document",
        ContentCategory::Image => "content-image",
        ContentCategory::Audio => "content-audio",
        ContentCategory::Video => "content-video",
        ContentCategory::Archive => "content-archive",
        ContentCategory::Executable => "content-executable",
        ContentCategory::Font => "content-font",
        ContentCategory::Binary => "content-binary",
    }
}

fn uppercase_extension(name: &OsStr) -> Option<String> {
    let ext = Path::new(name).extension()?.to_str()?.trim();
    (!ext.is_empty()).then(|| ext.to_ascii_uppercase())
}

fn format_ext_file(catalog: &Catalog, ext: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("ext", ext);
    catalog.message("detailed-type-extension-file", Some(&args))
}

fn format_source_code(catalog: &Catalog, language: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("language", language);
    catalog.message("detailed-type-source-code", Some(&args))
}

/// Fluent message IDs for common MIME types whose translations are missing in
/// `shared-mime-info` 2.4. Only consulted after per-type XML lookup misses the
/// active GDS interface language.
fn fluent_mime_fallback_key(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "text/plain" => "mime-fallback-text-plain",
        "application/octet-stream" => "mime-fallback-application-octet-stream",
        "application/epub+zip" => "mime-fallback-application-epub-zip",
        "application/x-executable" => "mime-fallback-application-x-executable",
        "application/x-sharedlib" => "mime-fallback-application-x-sharedlib",
        "application/x-shellscript" => "mime-fallback-application-x-shellscript",
        "application/x-desktop" => "mime-fallback-application-x-desktop",
        "application/wasm" => "mime-fallback-application-wasm",
        "application/vnd.efi.iso" => "mime-fallback-application-vnd-efi-iso",
        "application/vnd.microsoft.portable-executable" => {
            "mime-fallback-application-vnd-microsoft-portable-executable"
        }
        "application/x-dosexec" => "mime-fallback-application-x-dosexec",
        "application/x-bzip2" => "mime-fallback-application-x-bzip2",
        "application/x-bzip2-compressed-tar" => "mime-fallback-application-x-bzip2-compressed-tar",
        "application/x-powershell" => "mime-fallback-application-x-powershell",
        "application/schema+json" => "mime-fallback-application-schema-json",
        "application/json5" => "mime-fallback-application-json5",
        "application/x-zerosize" => "mime-fallback-application-x-zerosize",
        "image/svg+xml-compressed" => "mime-fallback-image-svg-xml-compressed",
        "application/vnd.comicbook+zip" => "mime-fallback-application-vnd-comicbook-zip",
        "application/vnd.comicbook-rar" => "mime-fallback-application-vnd-comicbook-rar",
        _ => return None,
    })
}

pub(crate) fn format_size(bytes: u64) -> String {
    const KIB: f64 = 1024.;
    let bytes = bytes as f64;
    if bytes >= KIB * KIB * KIB {
        format!("{:.1} GiB", bytes / (KIB * KIB * KIB))
    } else if bytes >= KIB * KIB {
        format!("{:.1} MiB", bytes / (KIB * KIB))
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

#[cfg(test)]
mod tests;
