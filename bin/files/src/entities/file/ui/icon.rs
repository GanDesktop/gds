//! File icons shared by the file table, grid tiles, status rows, and drag preview.
//!
//! Icon choice is presentation only. A symbolic link keeps `EntryKind::SymbolicLink`
//! so drops, renames and deletions still act on the link, while its icon shows the
//! target category with a small link badge. Only a missing target uses the
//! broken-chain icon; an uninspectable target keeps a link badge of its own.

use std::{path::Path, sync::OnceLock};

use directories::{BaseDirs, UserDirs};
use gds::ui::IconName;

use crate::entities::file::directory::{
    ContentCategory, ContentType, Entry, EntryKind, LinkTarget,
};

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

    /// Whether `entry` displays with directory styling (directories and resolved directory links).
    pub(crate) fn is_directory(entry: &Entry) -> bool {
        entry.kind == EntryKind::Directory
            || (entry.kind == EntryKind::SymbolicLink
                && entry.details.target.as_ref().is_some_and(|target| {
                    matches!(
                        target,
                        LinkTarget::Resolved {
                            kind: EntryKind::Directory,
                            ..
                        }
                    )
                }))
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
        static USE_DEFAULT_NAMES: OnceLock<bool> = OnceLock::new();
        let use_default_names = *USE_DEFAULT_NAMES.get_or_init(|| {
            BaseDirs::new().is_some_and(|base| {
                base.config_dir()
                    .join("user-dirs.dirs")
                    .try_exists()
                    .is_ok_and(|exists| !exists)
            })
        });
        let relative = path.strip_prefix(dirs.home_dir()).ok();
        [
            (dirs.desktop_dir(), "Desktop", IconName::Monitor),
            (dirs.document_dir(), "Documents", IconName::FileText),
            (dirs.download_dir(), "Downloads", IconName::FolderDown),
            (dirs.picture_dir(), "Pictures", IconName::Images),
            (dirs.audio_dir(), "Music", IconName::Music),
            (dirs.video_dir(), "Videos", IconName::Video),
            (dirs.public_dir(), "Public", IconName::Users),
            (dirs.template_dir(), "Templates", IconName::LayoutTemplate),
        ]
        .into_iter()
        .find_map(|(directory, default_name, icon)| {
            let matches = directory.map_or_else(
                || use_default_names && relative == Some(Path::new(default_name)),
                |directory| path == directory,
            );
            matches.then_some(icon)
        })
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

    pub(crate) fn category_icon(category: Option<ContentCategory>) -> IconName {
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
