//! File icons shared by the file table, grid tiles, status rows, and drag preview.
//!
//! Icon choice is presentation only. A symbolic link keeps `EntryKind::SymbolicLink`
//! so drops, renames and deletions still act on the link, while its icon shows the
//! target category with a small link badge. Only a missing target uses the
//! broken-chain icon; an uninspectable target keeps a link badge of its own.

use std::{path::Path, sync::OnceLock};

use directories::UserDirs;
use gds_uim::{component, el};
use gpui_kit::IntoElement;
use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme, Icon},
    prelude::*,
    *,
};

use crate::entities::file::directory::{
    ContentCategory, ContentType, Entry, EntryKind, LinkTarget,
};

/// Size of the corner badge relative to the base icon.
const BADGE_SIZE_FRACTION: f32 = 0.55;
/// The badge sits on the icon's bottom-right corner, just outside it, so the
/// target glyph stays readable.
const BADGE_INSET: f32 = -4.;

/// Icon for one entry: a base icon plus an optional corner badge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FileIcon {
    base: IconName,
    badge: Option<IconName>,
}

impl FileIcon {
    /// Icon describing `entry`, using its target for symbolic links.
    pub(crate) fn for_entry(entry: &Entry) -> Self {
        match entry.kind {
            EntryKind::Directory => Self::plain(Self::directory_icon(&entry.path)),
            EntryKind::File => Self::for_content(entry.details.content.as_ref()),
            EntryKind::SymbolicLink => Self::for_link(entry),
            EntryKind::Other => Self::plain(IconName::File),
            EntryKind::Unknown => Self::plain(IconName::FileQuestionMark),
        }
    }

    /// Icon describing `path` without counting directory children or reading file headers.
    pub(crate) fn for_path(path: &Path) -> Self {
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            return Self::plain(IconName::FileQuestionMark);
        };
        match EntryKind::from(metadata.file_type()) {
            EntryKind::Directory => Self::plain(Self::directory_icon(path)),
            EntryKind::File => {
                let name = path.file_name().unwrap_or(path.as_os_str());
                let content = ContentType::from_filename(name);
                Self::for_content(content.as_ref())
            }
            EntryKind::SymbolicLink => {
                let (target, _) = LinkTarget::from_path_base(path);
                Self::for_link_target(path, &target)
            }
            EntryKind::Other => Self::plain(IconName::File),
            EntryKind::Unknown => Self::plain(IconName::FileQuestionMark),
        }
    }

    /// Color follows the entry's category, independently of its chosen glyph.
    pub(crate) fn color(entry: &Entry, cx: &App) -> Hsla {
        let directory = entry.kind == EntryKind::Directory
            || (entry.kind == EntryKind::SymbolicLink
                && entry.details.target.as_ref().is_some_and(|target| {
                    matches!(
                        target,
                        LinkTarget::Resolved {
                            kind: EntryKind::Directory,
                            ..
                        }
                    )
                }));
        if directory {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        }
    }

    /// Icon for a drag that carries several entries.
    pub(crate) fn multiple() -> Self {
        Self::plain(IconName::Files)
    }

    /// The base icon, before the corner badge.
    pub(crate) fn base(self) -> IconName {
        self.base
    }

    /// The corner badge, present only when the entry is a symbolic link.
    pub(crate) fn badge(self) -> Option<IconName> {
        self.badge
    }

    fn directory_icon(path: &Path) -> IconName {
        static DIRECTORIES: OnceLock<Option<UserDirs>> = OnceLock::new();
        let Some(dirs) = DIRECTORIES.get_or_init(UserDirs::new) else {
            return IconName::Folder;
        };
        Self::user_directory_icon(path, dirs)
    }

    fn user_directory_icon(path: &Path, dirs: &UserDirs) -> IconName {
        if path == dirs.home_dir() {
            return IconName::House;
        }
        [
            (dirs.desktop_dir(), IconName::Monitor),
            (dirs.document_dir(), IconName::FileText),
            (dirs.download_dir(), IconName::FolderDown),
            (dirs.picture_dir(), IconName::Images),
            (dirs.audio_dir(), IconName::Music),
            (dirs.video_dir(), IconName::Video),
            (dirs.public_dir(), IconName::Users),
            (dirs.template_dir(), IconName::LayoutTemplate),
        ]
        .into_iter()
        .find_map(|(directory, icon)| (directory == Some(path)).then_some(icon))
        .unwrap_or(IconName::Folder)
    }

    fn for_content(content: Option<&ContentType>) -> Self {
        Self::plain(Self::category_icon(content.map(|content| content.category)))
    }

    fn for_link(entry: &Entry) -> Self {
        let Some(target) = entry.details.target.as_ref() else {
            return Self::plain(IconName::Link);
        };
        Self::for_link_target(&entry.path, target)
    }

    fn for_link_target(path: &Path, target: &LinkTarget) -> Self {
        match target {
            // A dangling link is the only state that shows the broken-chain icon.
            LinkTarget::Broken => Self::plain(IconName::Unlink),
            // The target exists as far as the path goes but could not be inspected;
            // that is not the same as a missing target, so keep the link badge.
            LinkTarget::Unreachable => Self::badged(IconName::FileQuestionMark, IconName::Link),
            LinkTarget::Resolved { kind, content } => {
                let base = match kind {
                    EntryKind::Directory => Self::directory_icon(path),
                    EntryKind::File => {
                        Self::category_icon(content.as_ref().map(|content| content.category))
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

    pub(crate) fn plain(base: IconName) -> Self {
        Self { base, badge: None }
    }

    fn badged(base: IconName, badge: IconName) -> Self {
        Self {
            base,
            badge: Some(badge),
        }
    }
}

/// The artwork slot used by file rows, tiles and drag previews.
#[component]
#[derive(IntoElement)]
pub(crate) struct FileArtwork {
    #[attr]
    icon: FileIcon,
    #[attr]
    size: Pixels,
    #[attr(option)]
    color: Option<Hsla>,
}

impl Default for FileArtwork {
    fn default() -> Self {
        Self {
            icon: FileIcon::plain(IconName::File),
            size: px(gds::appearance::tokens::ICON_SIZE),
            color: None,
        }
    }
}

impl RenderOnce for FileArtwork {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or_else(|| window.text_style().color);
        let badge_size = self.size * BADGE_SIZE_FRACTION;
        el! {
            .div { relative; flex_shrink_0; w: $(self.size); h: $(self.size); } [
                $(self.icon.base()).Icon::new { size: $(self.size); text_color: color; },
                @if (let Some(badge) = self.icon.badge())
                    .div { absolute; bottom: BADGE_INSET.px; right: BADGE_INSET.px; flex; } [
                        badge.Icon::new { size: badge_size; text_color: color; },
                    ],
                @end,
            ]
        }
    }
}
