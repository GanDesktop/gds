//! Formatted labels and size strings for file entries.

use gds::i18n::{self, t};

use crate::entities::file::directory::{ChildCount, ContentCategory, Entry, EntryKind, LinkTarget};

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
                let target_label: String = match kind {
                    EntryKind::Directory => i18n::text("kind-directory"),
                    EntryKind::File => entry.details.display_content().map_or_else(
                        || i18n::text("kind-file"),
                        |content| Self::category_label(content.category),
                    ),
                    EntryKind::Other => i18n::text("kind-other"),
                    EntryKind::Unknown => i18n::text("kind-unknown"),
                    EntryKind::SymbolicLink => i18n::text("kind-link"),
                };
                t!("type-link", target = target_label.to_string())
            }
        }
    }

    /// Concrete format name, MIME fallback, or an explicit unknown value.
    pub(crate) fn detailed_type(entry: &Entry) -> String {
        if entry.kind == EntryKind::Directory {
            return i18n::text("kind-directory");
        }
        let Some(content) = entry.details.display_content() else {
            return UNKNOWN_VALUE.into();
        };
        match content.label() {
            Some(label) => label.into(),
            None => i18n::text("kind-unknown"),
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

    fn category_label(category: ContentCategory) -> String {
        let id = match category {
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
        };
        i18n::text(id)
    }

    /// Byte size for the details panel; a directory has no byte size there.
    pub(crate) fn size(entry: &Entry) -> String {
        if entry.kind == EntryKind::Directory {
            UNKNOWN_VALUE.to_owned()
        } else {
            entry
                .size
                .map(format_size)
                .unwrap_or_else(|| UNKNOWN_VALUE.into())
        }
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
