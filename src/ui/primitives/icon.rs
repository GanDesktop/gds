//! Embedded Lucide 1.43.0, Tabler and window-control SVG icons rendered via `zgui` `<vector>`.
//! See `src/ui/icon/LICENSE` for upstream notices.

use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::vocab::PropValue;
use zgui::{component, view};

macro_rules! icons {
    ($($variant:ident => $asset:literal),+ $(,)?) => {
        /// Named monochrome vector icon embedded at compile time.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum IconName {
            $($variant,)+
        }

        impl IconName {
            /// Every bundled icon variant.
            pub const ALL: &'static [Self] = &[
                $(Self::$variant,)+
            ];

            /// Kebab-case identifier matching the SVG asset filename.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $asset,)+
                }
            }

            /// Embedded SVG document using `currentColor` inheritance.
            #[must_use]
            pub const fn svg(self) -> &'static str {
                match self {
                    $(Self::$variant => include_str!(concat!("icon/assets/", $asset, ".svg")),)+
                }
            }
        }
    };
}

icons! {
    Activity => "activity",
    Archive => "archive",
    ArchiveRestore => "archive-restore",
    ArrowDown => "arrow-down",
    ArrowDownWideNarrow => "arrow-down-wide-narrow",
    ArrowLeft => "arrow-left",
    ArrowRight => "arrow-right",
    ArrowUp => "arrow-up",
    ArrowUpRight => "arrow-up-right",
    BookOpen => "book-open",
    Bookmark => "bookmark",
    Bot => "bot",
    Broom => "broom",
    Cable => "cable",
    Check => "check",
    ChevronDown => "chevron-down",
    ChevronRight => "chevron-right",
    ChevronUp => "chevron-up",
    Circle => "circle",
    CircleAlert => "circle-alert",
    CircleArrowUp => "circle-arrow-up",
    CircleCheck => "circle-check",
    CircleNumber1 => "circle-number-1",
    CircleQuestionMark => "circle-question-mark",
    CircleX => "circle-x",
    Clipboard => "clipboard",
    ClipboardCopy => "clipboard-copy",
    ClipboardPaste => "clipboard-paste",
    Clock => "clock",
    Close => "close",
    Columns3Cog => "columns-3-cog",
    Copy => "copy",
    CopyPlus => "copy-plus",
    CornerLeftUp => "corner-left-up",
    Crop => "crop",
    Disc => "disc",
    Download => "download",
    Droplet => "droplet",
    Ellipsis => "ellipsis",
    ExternalLink => "external-link",
    Eye => "eye",
    File => "file",
    FileArchive => "file-archive",
    FileCode => "file-code",
    FileImage => "file-image",
    FileMusic => "file-music",
    FilePen => "file-pen",
    FileQuestionMark => "file-question-mark",
    FileTerminal => "file-terminal",
    FileText => "file-text",
    FileType => "file-type",
    FileVideoCamera => "file-video-camera",
    Files => "files",
    FlipHorizontal2 => "flip-horizontal-2",
    FlipVertical2 => "flip-vertical-2",
    Folder => "folder",
    FolderDown => "folder-down",
    FolderInput => "folder-input",
    FolderOpen => "folder-open",
    FolderPlus => "folder-plus",
    Gauge => "gauge",
    Github => "github",
    Globe => "globe",
    Grid2x2 => "grid-2x2",
    GripVertical => "grip-vertical",
    Hand => "hand",
    HardDrive => "hard-drive",
    House => "house",
    Image => "image",
    ImageOff => "image-off",
    Images => "images",
    Info => "info",
    LayersArrowDown => "layers-arrow-down",
    LayersArrowUp => "layers-arrow-up",
    LayoutGrid => "layout-grid",
    LayoutTemplate => "layout-template",
    Lightbulb => "lightbulb",
    Link => "link",
    List => "list",
    ListFilter => "list-filter",
    ListTodo => "list-todo",
    ListX => "list-x",
    Loader => "loader",
    LoaderCircle => "loader-circle",
    Lock => "lock",
    LogIn => "log-in",
    LogOut => "log-out",
    Maximize => "maximize",
    Maximize2 => "maximize-2",
    Menu => "menu",
    MessageCircle => "message-circle",
    MessageSquare => "message-square",
    MessagesSquare => "messages-square",
    Minimize2 => "minimize-2",
    Minus => "minus",
    Monitor => "monitor",
    MousePointer => "mouse-pointer",
    Music => "music",
    Network => "network",
    Palette => "palette",
    PanelLeft => "panel-left",
    PanelRight => "panel-right",
    Pause => "pause",
    Pencil => "pencil",
    Pin => "pin",
    Play => "play",
    Plug => "plug",
    Plus => "plus",
    Redo => "redo",
    Redo2 => "redo-2",
    RefreshCw => "refresh-cw",
    RotateCcw => "rotate-ccw",
    RotateCw => "rotate-cw",
    Save => "save",
    Scale => "scale",
    ScanSearch => "scan-search",
    Scissors => "scissors",
    Search => "search",
    Send => "send",
    Server => "server",
    Settings => "settings",
    Shield => "shield",
    ShieldCheck => "shield-check",
    Sparkles => "sparkles",
    Square => "square",
    Star => "star",
    StarOff => "star-off",
    Tag => "tag",
    Terminal => "terminal",
    TextCursor => "text-cursor",
    Trash => "trash",
    TriangleAlert => "triangle-alert",
    Type => "type",
    Undo => "undo",
    Undo2 => "undo-2",
    UnfoldHorizontal => "unfold-horizontal",
    UnfoldVertical => "unfold-vertical",
    Unlink => "unlink",
    Unplug => "unplug",
    Upload => "upload",
    Usb => "usb",
    User => "user",
    Users => "users",
    Video => "video",
    Volume2 => "volume-2",
    WindowClose => "window-close",
    WindowMaximize => "window-maximize",
    WindowMinimize => "window-minimize",
    WindowRestore => "window-restore",
    Workflow => "workflow",
    Wrench => "wrench",
    X => "x",
    Zap => "zap",
    ZoomIn => "zoom-in",
    ZoomOut => "zoom-out",
}

/// Renders one monochrome vector icon whose stroke/fill inherits CSS `color`.
#[component]
pub fn Icon(
    /// Which icon to draw.
    #[prop(into)]
    name: Signal<IconName, LocalStorage>,
    /// Square icon side length in logical CSS pixels.
    #[prop(default = crate::appearance::tokens::ICON_SIZE)]
    size: f32,
    /// Optional accessible name; hidden from assistive technologies when omitted.
    #[prop(into, optional)]
    label: Option<String>,
    /// Optional reference to the underlying `<vector>` node.
    #[prop(optional)]
    node_ref: Option<NodeRef>,
    /// Extra CSS classes appended after `.gds-icon`.
    #[prop(into, optional)]
    class: Classes,
    /// Forwarded element attributes, event listeners, and accessibility bindings.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    crate::ui::styles::install_ui_stylesheets();
    let semantics = Attrs::new().a11y_from(match label {
        Some(text) => A11yBinding::new(Role::Image).label(text),
        None => A11yBinding::new(Role::Image).hidden(true),
    });
    let size_px = format!("{size}px");
    view! {
        vector(
            class = "gds-icon",
            node_ref = node_ref.unwrap_or_default(),
            var:--gds-icon-size = size_px,
            attr:data-icon = move || Some(name.get().as_str().to_owned()),
            prop:svg = move || PropValue::from(name.get().svg()),
            {..semantics},
            {..attrs},
            class = class
        )
    }
}

impl IntoView for IconName {
    type Output = AnyView;

    fn into_view(self) -> Self::Output {
        AnyView::new(view! { Icon(name = self) })
    }
}

#[cfg(test)]
mod tests {
    use super::IconName;

    #[test]
    fn every_bundled_icon_is_valid_current_color_svg() {
        for &icon in IconName::ALL {
            let svg = icon.svg();
            assert!(!svg.is_empty(), "empty svg for {}", icon.as_str());
            assert!(svg.contains("<svg"), "missing <svg for {}", icon.as_str());
            assert!(
                svg.contains("currentColor"),
                "missing currentColor for {}",
                icon.as_str()
            );
        }
    }
}
