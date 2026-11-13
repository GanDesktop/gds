//! Formatted labels and size strings for file entries.

use gds::i18n::{self, t};
use gpui_kit::{AnyView, App, SharedString, Window, component::tooltip::Tooltip};

use crate::entities::file::directory::{ChildCount, ContentCategory, Entry, EntryKind, LinkTarget};

/// Shown for a metadata value the platform did not provide.
pub(crate) const UNKNOWN_VALUE: &str = "—";

/// Formats the non-icon cells of a file row; the details view shares these.
pub(crate) struct EntryPresentation;

/// Owner cell contents; the current user's name is emphasized.
pub(crate) struct OwnerLabel {
    pub(crate) text: SharedString,
    pub(crate) current: bool,
}

impl EntryPresentation {
    pub(crate) fn name_hint(name: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView {
        move |window, cx| Tooltip::new(name.clone()).build(window, cx)
    }

    pub(crate) fn owner(entry: &Entry, cx: &App) -> OwnerLabel {
        let Some(owner) = entry.details.owner.as_ref() else {
            return OwnerLabel {
                text: UNKNOWN_VALUE.into(),
                current: false,
            };
        };
        let name = owner.label().into_owned();
        if !owner.is_current_user {
            return OwnerLabel {
                text: name.into(),
                current: false,
            };
        }
        OwnerLabel {
            text: t!(@cx, "owner-you", name),
            current: true,
        }
    }

    pub(crate) fn group(entry: &Entry) -> SharedString {
        entry.details.group.as_ref().map_or_else(
            || UNKNOWN_VALUE.into(),
            |group| group.label().into_owned().into(),
        )
    }

    /// Localized broad type, or the link relationship for a symbolic link.
    pub(crate) fn type_label(entry: &Entry, cx: &App) -> SharedString {
        if entry.kind == EntryKind::Directory {
            return i18n::text("kind-directory", cx);
        }
        if entry.kind != EntryKind::SymbolicLink {
            return Self::kind_label(entry, cx);
        }
        let Some(target) = entry.details.target.as_ref() else {
            return i18n::text("kind-link", cx);
        };
        match target {
            LinkTarget::Broken => i18n::text("link-broken", cx),
            LinkTarget::Unreachable => i18n::text("link-unreachable", cx),
            LinkTarget::Resolved { kind, .. } => {
                let target_label: SharedString = match kind {
                    EntryKind::Directory => i18n::text("kind-directory", cx),
                    EntryKind::File => entry.details.display_content().map_or_else(
                        || i18n::text("kind-file", cx),
                        |content| Self::category_label(content.category, cx),
                    ),
                    EntryKind::Other => i18n::text("kind-other", cx),
                    EntryKind::Unknown => i18n::text("kind-unknown", cx),
                    EntryKind::SymbolicLink => i18n::text("kind-link", cx),
                };
                t!(@cx, "type-link", target = target_label.to_string())
            }
        }
    }

    /// Concrete format name, MIME fallback, or an explicit unknown value.
    pub(crate) fn detailed_type(entry: &Entry, cx: &App) -> SharedString {
        if entry.kind == EntryKind::Directory {
            return i18n::text("kind-directory", cx);
        }
        let Some(content) = entry.details.display_content() else {
            return UNKNOWN_VALUE.into();
        };
        match content.label() {
            Some(label) => label.into(),
            None => i18n::text("kind-unknown", cx),
        }
    }

    fn kind_label(entry: &Entry, cx: &App) -> SharedString {
        match entry.kind {
            EntryKind::Directory => i18n::text("kind-directory", cx),
            EntryKind::File => entry.details.display_content().map_or_else(
                || i18n::text("kind-file", cx),
                |content| Self::category_label(content.category, cx),
            ),
            EntryKind::SymbolicLink => i18n::text("kind-link", cx),
            EntryKind::Other => i18n::text("kind-other", cx),
            EntryKind::Unknown => i18n::text("kind-unknown", cx),
        }
    }

    fn category_label(category: ContentCategory, cx: &App) -> SharedString {
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
        i18n::text(id, cx)
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
    pub(crate) fn size_label(entry: &Entry, cx: &App) -> SharedString {
        if entry.kind == EntryKind::Directory {
            return match entry.details.children {
                Some(children) => Self::child_count_label(children, cx),
                None => UNKNOWN_VALUE.into(),
            };
        }
        entry
            .size
            .map_or_else(|| UNKNOWN_VALUE.into(), |bytes| format_size(bytes).into())
    }

    /// A count that stopped at the read limit is shown as a lower bound so the
    /// cell never claims a precision it does not have.
    fn child_count_label(children: ChildCount, cx: &App) -> SharedString {
        let id = if children.truncated {
            "directory-item-count-at-least"
        } else {
            "directory-item-count"
        };
        i18n::count(id, children.count as usize, cx)
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
