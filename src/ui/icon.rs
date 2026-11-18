//! Lucide 1.43.0 icons from GPUI Kit's f61ebe1 asset bundle.
//! See icon/LICENSE for the upstream notices.
use std::{fmt, sync::OnceLock};

use anti_webui::{core::svg::MonochromeSvg, prelude::*};

/// Embedded monochrome vector artwork with shared geometry and inherited text color.
///
/// ```ignore
/// use gds::ui::icon::SvgAsset;
///
/// static MARK: SvgAsset = SvgAsset::new(include_bytes!("assets/mark.svg"));
/// let mark = MARK.view();
/// ```
pub struct SvgAsset {
    bytes: &'static [u8],
    artwork: OnceLock<MonochromeSvg>,
}

impl SvgAsset {
    pub const fn new(bytes: &'static [u8]) -> Self {
        Self {
            bytes,
            artwork: OnceLock::new(),
        }
    }

    pub fn view(&self) -> Svg {
        self.artwork
            .get_or_init(|| MonochromeSvg::parse(self.bytes).expect("bundled SVG artwork is valid"))
            .view()
    }
}

impl fmt::Debug for SvgAsset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("SvgAsset").finish_non_exhaustive()
    }
}

macro_rules! icons {
    ($($($variant:ident)|+ => $asset:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon {
            $($($variant,)+)+
        }

        impl Icon {
            fn artwork(self) -> &'static SvgAsset {
                match self {
                    $($(Self::$variant)|+ => {
                        static ARTWORK: SvgAsset = SvgAsset::new(include_bytes!(concat!("icon/assets/", $asset, ".svg")));
                        &ARTWORK
                    },)+
                }
            }
        }
    };
}

icons! {
    Back => "arrow-left",
    Forward => "arrow-right",
    Up => "arrow-up",
    Refresh => "refresh-cw",
    Search => "search",
    Menu => "menu",
    Settings => "settings",
    Help => "circle-question-mark",
    Close => "x",
    Add => "plus",
    Folder => "folder",
    FolderPlus => "folder-plus",
    TextCursor => "text-cursor",
    File => "file",
    Files => "files",
    Grid => "layout-grid",
    List => "list",
    Copy => "copy",
    Cut => "scissors",
    Paste => "clipboard",
    Trash => "trash",
    Undo => "undo-2",
    Redo => "redo-2",
    Home => "house",
    Star => "star",
    Terminal => "terminal",
    Details | Info => "info",
    Hidden => "eye",
    Check => "check",
    CircleCheck => "circle-check",
    CircleClose => "circle-x",
    More => "ellipsis",
    Network => "network",
    Drive => "hard-drive",
    Usb => "usb",
    Disc => "disc",
    Tag => "tag",
    ChevronDown => "chevron-down",
    ChevronUp => "chevron-up",
    ChevronRight => "chevron-right",
    Sort => "arrow-down-wide-narrow",
    Edit => "pencil",
    Recent | Clock => "clock",
    Link => "link",
    BrokenLink => "unlink",
    Unknown => "file-question-mark",
    Image => "image",
    Audio => "music",
    Video => "video",
    Archive => "file-archive",
    Code => "file-code",
    Document => "file-text",
    FileType => "file-type",
    FileImage => "file-image",
    FileMusic => "file-music",
    FileVideo => "file-video-camera",
    Font => "type",
    Executable => "file-terminal",
    Minimize => "minus",
    Maximize => "square",
    Pin => "pin",
    Sidebar => "panel-left",
    Columns => "columns-3-cog",
    Palette => "palette",
    Gauge => "gauge",
    Plug => "plug",
    Warning => "triangle-alert",
    Scale => "scale",
    Tasks => "list-todo",
    Monitor => "monitor",
    FolderDown => "folder-down",
    Upload => "upload",
    Download => "download",
    FolderInput => "folder-input",
    Expand => "maximize-2",
    Contract => "minimize-2",
    Reset => "rotate-ccw",
    Images => "images",
    Users => "users",
    LayoutTemplate => "layout-template",
    FolderOpen => "folder-open",
    Unplug => "unplug",
    ExternalLink => "external-link",
    ClipboardCopy => "clipboard-copy",
    Lock => "lock",
    CircleAlert => "circle-alert",
}

impl Icon {
    pub fn view(self, size: f32) -> Svg {
        self.artwork().view().size(size).flex_none()
    }
}

impl IntoView for Icon {
    fn into_view(self) -> View {
        self.view(crate::appearance::tokens::ICON_SIZE).into_view()
    }
}
