//! File icons shared by the file table and the drag preview.
//!
//! Icon choice is presentation only. A symbolic link keeps `EntryKind::SymbolicLink`
//! so drops, renames and deletions still act on the link, while its icon shows the
//! target category with a small link badge. Only a missing target uses the
//! broken-chain icon; an uninspectable target keeps a link badge of its own.

use gds_uim::el;
use gpui_kit::{assets::IconName, component::Icon, prelude::*, *};

use crate::entities::file::directory::{
    ContentCategory, ContentType, Entry, EntryKind, LinkStatus,
};

/// Size of the corner badge relative to the base icon.
const BADGE_SIZE_FRACTION: f32 = 0.55;
/// The badge sits on the icon's bottom-right corner, just outside it, so the
/// target glyph stays readable.
const BADGE_INSET: f32 = -4.;

/// Icon for one entry: a base icon plus an optional corner badge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FileIcon {
    base: IconName,
    badge: Option<IconName>,
}

impl FileIcon {
    /// Icon describing `entry`, using its target for symbolic links.
    pub(super) fn for_entry(entry: &Entry) -> Self {
        match entry.kind {
            EntryKind::Directory => Self::plain(IconName::Folder),
            EntryKind::File => Self::for_content(entry.details.content.as_ref()),
            EntryKind::SymbolicLink => Self::for_link(entry),
            EntryKind::Other => Self::plain(IconName::File),
            EntryKind::Unknown => Self::plain(IconName::FileQuestionMark),
        }
    }

    /// Icon for a drag that carries several entries.
    pub(super) fn multiple() -> Self {
        Self::plain(IconName::Files)
    }

    /// The base icon, before the corner badge.
    pub(super) fn base(self) -> IconName {
        self.base
    }

    /// The corner badge, present only when the entry is a symbolic link.
    pub(super) fn badge(self) -> Option<IconName> {
        self.badge
    }

    /// Draw the icon at `size`, with the badge in its bottom-right corner.
    pub(super) fn render(self, size: Pixels, color: Hsla) -> AnyElement {
        let badge_size = size * BADGE_SIZE_FRACTION;
        el! {
            .div { relative; flex_shrink_0; w: $size; h: $size; } [
                $(self.base).Icon::new { size: $size; text_color: $color },
                @if (let Some(badge) = self.badge())
                    .div {
                        absolute; bottom: $(px(BADGE_INSET)); right: $(px(BADGE_INSET)); flex;
                    } [
                        badge.Icon::new { size: $badge_size; text_color: $color },
                    ],
                @end,
            ]
        }
        .into_any_element()
    }

    fn for_content(content: Option<&ContentType>) -> Self {
        Self::plain(Self::category_icon(content.map(|content| content.category)))
    }

    fn for_link(entry: &Entry) -> Self {
        let Some(target) = entry.details.target.as_ref() else {
            return Self::plain(IconName::Link);
        };
        match target.status {
            // A dangling link is the only state that shows the broken-chain icon.
            LinkStatus::Broken => Self::plain(IconName::Unlink),
            // The target exists as far as the path goes but could not be inspected;
            // that is not the same as a missing target, so keep the link badge.
            LinkStatus::Unreachable => Self::badged(IconName::FileQuestionMark, IconName::Link),
            LinkStatus::Resolved => {
                let base = match target.kind {
                    EntryKind::Directory => IconName::Folder,
                    EntryKind::File => {
                        Self::category_icon(target.content.as_ref().map(|content| content.category))
                    }
                    EntryKind::Other => IconName::File,
                    EntryKind::Unknown => IconName::FileQuestionMark,
                    EntryKind::SymbolicLink => IconName::Link,
                };
                Self::badged(base, IconName::Link)
            }
        }
    }

    fn category_icon(category: Option<ContentCategory>) -> IconName {
        match category {
            Some(ContentCategory::Text) => IconName::FileText,
            Some(ContentCategory::Code) => IconName::FileCode,
            Some(ContentCategory::Document) => IconName::FileType,
            Some(ContentCategory::Image) => IconName::FileImage,
            Some(ContentCategory::Audio) => IconName::FileMusic,
            Some(ContentCategory::Video) => IconName::FileVideoCamera,
            Some(ContentCategory::Archive) => IconName::FileArchive,
            Some(ContentCategory::Executable) => IconName::FileTerminal,
            Some(ContentCategory::Font) => IconName::Type,
            Some(ContentCategory::Binary) | None => IconName::File,
        }
    }

    fn plain(base: IconName) -> Self {
        Self { base, badge: None }
    }

    fn badged(base: IconName, badge: IconName) -> Self {
        Self {
            base,
            badge: Some(badge),
        }
    }
}
