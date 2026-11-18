//! File icons shared by the file table, grid tiles, status rows, and drag preview.
//!
//! Icon choice is presentation only. A symbolic link keeps `EntryKind::SymbolicLink`
//! so drops, renames and deletions still act on the link, while its icon shows the
//! target category with a small link badge. Only a missing target uses the
//! broken-chain icon; an uninspectable target keeps a link badge of its own.

use std::{path::Path, sync::OnceLock};

use anti_webui::prelude::*;
use directories::{BaseDirs, UserDirs};
use gds::ui::Icon;
use gds_uim::{component, el};

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
    base: Icon,
    badge: Option<Icon>,
}

impl FileIcon {
    /// Icon describing `entry`, using its target for symbolic links.
    pub(crate) fn for_entry(entry: &Entry) -> Self {
        match entry.kind {
            EntryKind::Directory => Self::plain(Self::directory_icon(&entry.path)),
            EntryKind::File => Self::for_content(entry.details.content.as_ref()),
            EntryKind::SymbolicLink => Self::for_link(entry),
            EntryKind::Other => Self::plain(Icon::File),
            EntryKind::Unknown => Self::plain(Icon::Unknown),
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

    /// Icon tint corresponding to whether the entry resolves to a directory.
    pub(crate) fn category_color(directory: bool) -> Color {
        if directory {
            theme().accent
        } else {
            theme().text_muted
        }
    }

    /// The base icon, before the corner badge.
    pub(crate) fn base(self) -> Icon {
        self.base
    }

    fn directory_icon(path: &Path) -> Icon {
        static DIRECTORIES: OnceLock<Option<UserDirs>> = OnceLock::new();
        let Some(dirs) = DIRECTORIES.get_or_init(UserDirs::new) else {
            return Icon::Folder;
        };
        Self::user_directory_icon(path, dirs)
    }

    fn user_directory_icon(path: &Path, dirs: &UserDirs) -> Icon {
        if path == dirs.home_dir() {
            return Icon::Home;
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
            (dirs.desktop_dir(), "Desktop", Icon::Monitor),
            (dirs.document_dir(), "Documents", Icon::Document),
            (dirs.download_dir(), "Downloads", Icon::FolderDown),
            (dirs.picture_dir(), "Pictures", Icon::Images),
            (dirs.audio_dir(), "Music", Icon::Audio),
            (dirs.video_dir(), "Videos", Icon::Video),
            (dirs.public_dir(), "Public", Icon::Users),
            (dirs.template_dir(), "Templates", Icon::LayoutTemplate),
        ]
        .into_iter()
        .find_map(|(directory, default_name, icon)| {
            let matches = directory.map_or_else(
                || use_default_names && relative == Some(Path::new(default_name)),
                |directory| path == directory,
            );
            matches.then_some(icon)
        })
        .unwrap_or(Icon::Folder)
    }

    fn for_content(content: Option<&ContentType>) -> Self {
        Self::plain(Self::category_icon(content.map(|content| content.category)))
    }

    fn for_link(entry: &Entry) -> Self {
        let Some(target) = entry.details.target.as_ref() else {
            return Self::plain(Icon::Link);
        };
        Self::for_link_target(&entry.path, target)
    }

    fn for_link_target(path: &Path, target: &LinkTarget) -> Self {
        match target {
            // A dangling link is the only state that shows the broken-chain icon.
            LinkTarget::Broken => Self::plain(Icon::BrokenLink),
            // The target exists as far as the path goes but could not be inspected;
            // that is not the same as a missing target, so keep the link badge.
            LinkTarget::Unreachable => Self::badged(Icon::Unknown, Icon::Link),
            LinkTarget::Resolved { kind, content } => {
                let base = match kind {
                    EntryKind::Directory => Self::directory_icon(path),
                    EntryKind::File => {
                        Self::category_icon(content.as_ref().map(|content| content.category))
                    }
                    EntryKind::Other => Icon::File,
                    EntryKind::Unknown => Icon::Unknown,
                    EntryKind::SymbolicLink => Icon::Link,
                };
                Self::badged(base, Icon::Link)
            }
        }
    }

    fn category_icon(category: Option<ContentCategory>) -> Icon {
        match category {
            Some(ContentCategory::Text) => Icon::Document,
            Some(ContentCategory::Code) => Icon::Code,
            Some(ContentCategory::Document) => Icon::FileType,
            Some(ContentCategory::Image) => Icon::FileImage,
            Some(ContentCategory::Audio) => Icon::FileMusic,
            Some(ContentCategory::Video) => Icon::FileVideo,
            Some(ContentCategory::Archive) => Icon::Archive,
            Some(ContentCategory::Executable) => Icon::Executable,
            Some(ContentCategory::Font) => Icon::Font,
            Some(ContentCategory::Binary) | None => Icon::File,
        }
    }

    pub(crate) fn plain(base: Icon) -> Self {
        Self { base, badge: None }
    }

    fn badged(base: Icon, badge: Icon) -> Self {
        Self {
            base,
            badge: Some(badge),
        }
    }
}

/// The shared file-row artwork with a distinct symbolic-link badge.
#[component]
pub(crate) struct FileArtwork {
    #[attr]
    icon: FileIcon,
    #[attr]
    size: f32,
    #[attr(option, option_setter = "color_opt")]
    color: Option<Color>,
}
impl Default for FileArtwork {
    fn default() -> Self {
        Self {
            icon: FileIcon::plain(Icon::File),
            size: gds::appearance::tokens::ICON_SIZE,
            color: None,
        }
    }
}
impl IntoView for FileArtwork {
    fn into_view(self) -> View {
        let color = self.color.unwrap_or_else(|| theme().text);
        let badge_size = self.size * BADGE_SIZE_FRACTION;
        el! {.div.{relative;flex_none;w:self->size;h:self->size;text_color:color}.[
            $(self.icon.base().view(self.size)),
            if let Some(badge)=self->icon->badge [
                .div.{absolute;bottom:BADGE_INSET;right:BADGE_INSET}.[$(badge.view(badge_size))],
            ],
        ]}
        .into_view()
    }
}
