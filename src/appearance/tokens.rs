//! Default GDS visual language, shared by every application and its reset actions.
//!
//! Dimensions are logical pixels; platform display scaling still applies. These
//! are defaults, not persisted suite-wide preferences. Application settings may
//! override typography and surface opacity independently.

use std::fmt::Write as _;
use std::time::Duration;

use zgui::canvas::zgui_color::Color;

pub const TOOLBAR_HEIGHT: f32 = 48.0;
pub const NOTIFICATION_INSET: f32 = 12.0;
pub const NOTIFICATION_TOP_GAP: f32 = 8.0;
pub const FOOTER_HEIGHT: f32 = 32.0;
pub const SIDEBAR_WIDTH: f32 = 224.0;
pub const COLLAPSED_SIDEBAR_WIDTH: f32 = 56.0;
pub const SIDEBAR_PADDING: f32 = 10.0;
/// Section labels sit inside the sidebar's padded row, aligned across sections.
pub const SIDEBAR_SECTION_PADDING: f32 = 6.0;
/// Standard horizontal inset for content rows and their headers.
pub const CONTENT_PADDING: f32 = 12.0;
/// Terminal window titles and the PTY grid share the host content-row edge.
pub const TERMINAL_CONTENT_INSET: f32 = CONTENT_PADDING;
pub const SIDEBAR_ITEM_HEIGHT: f32 = 36.0;
pub const ICON_BUTTON_SIZE: f32 = 32.0;
/// Dock actions retain a 4px breathing space above and below the standard target.
pub const TERMINAL_HEADER_HEIGHT: f32 = ICON_BUTTON_SIZE + 8.0;

pub const ICON_SIZE: f32 = 16.0;
/// Application identity fills more of the standard main-menu button.
pub const APPLICATION_MENU_ICON_SIZE: f32 = 24.0;
pub const SIDEBAR_ICON_SIZE: f32 = 18.0;
pub const SMALL_ICON_SIZE: f32 = 12.0;

/// The rem base, distinct from the 14px text used by ordinary UI controls.
pub const FONT_SIZE: f32 = 16.0;
pub const UI_TEXT_SCALE: f32 = 0.875;

/// Desktop defaults for surface backgrounds; text and icons stay opaque.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceOpacity {
    pub content: f32,
    pub sidebar: f32,
}

impl SurfaceOpacity {
    const FLOATING: Self = Self {
        content: 1.0,
        sidebar: 0.85,
    };
    const TILING: Self = Self {
        content: 0.85,
        sidebar: 0.5,
    };

    fn for_session(desktop: Option<&str>, has_tiling_socket: bool) -> Self {
        let tiling = match desktop.map(str::trim).filter(|name| !name.is_empty()) {
            Some(desktop) => desktop.split(':').any(|name| {
                [
                    "niri", "sway", "Hyprland", "river", "dwl", "qtile", "i3", "bspwm", "awesome",
                ]
                .iter()
                .any(|tiling| name.trim().eq_ignore_ascii_case(tiling))
            }),
            None => has_tiling_socket,
        };
        if tiling { Self::TILING } else { Self::FLOATING }
    }
}

impl Default for SurfaceOpacity {
    fn default() -> Self {
        if cfg!(any(target_os = "linux", target_os = "freebsd")) {
            let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
            let has_tiling_socket = ["NIRI_SOCKET", "SWAYSOCK", "HYPRLAND_INSTANCE_SIGNATURE"]
                .iter()
                .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
            Self::for_session(desktop.as_deref(), has_tiling_socket)
        } else {
            Self::FLOATING
        }
    }
}

/// Radius used when the user enables rounded floating windows.
pub const WINDOW_CORNER_RADIUS: f32 = 10.;

/// An alpha-capable window lets per-surface opacity changes apply immediately.
pub const WINDOW_BACKGROUND: Color = Color::TRANSPARENT;

pub const SMALL: f32 = FONT_SIZE * UI_TEXT_SCALE;
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(400);
/// Shared density and hierarchy for content lists, separate from navigation rows.
pub struct DataListStyle;

impl DataListStyle {
    pub const ROW_HEIGHT: f32 = 40.0;
    pub const SELECTION_HEIGHT: f32 = 36.0;
    pub const HEADER_HEIGHT: f32 = 30.0;
    pub const ROW_RADIUS: f32 = 6.0;
    pub const SELECTION_INSET: f32 = 2.0;
    pub const NAME_SIZE: f32 = 14.0;
    pub const META_SIZE: f32 = 12.0;
    pub const HEADER_SIZE: f32 = 12.0;
    pub const CELL_PADDING_X: f32 = CONTENT_PADDING;
    pub const CELL_PADDING_Y: f32 = 4.0;
    pub const SCROLLBAR_LANE: f32 = 16.0;

    #[must_use]
    pub const fn scaled_row_height(scale: f32) -> f32 {
        Self::ROW_HEIGHT * scale
    }

    #[must_use]
    pub const fn scaled_selection_height(scale: f32) -> f32 {
        Self::SELECTION_HEIGHT * scale
    }

    #[must_use]
    pub const fn scaled_header_height(scale: f32) -> f32 {
        Self::HEADER_HEIGHT * scale
    }
}

/// Write layout, icon, and list sizing `--gds-*` custom property declarations.
pub(crate) fn write_css_declarations(out: &mut String, font_scale: f32) {
    let fixed_pairs = [
        ("--gds-toolbar-height", TOOLBAR_HEIGHT),
        ("--gds-notification-inset", NOTIFICATION_INSET),
        ("--gds-notification-top-gap", NOTIFICATION_TOP_GAP),
        ("--gds-footer-height", FOOTER_HEIGHT),
        ("--gds-sidebar-width", SIDEBAR_WIDTH),
        ("--gds-collapsed-sidebar-width", COLLAPSED_SIDEBAR_WIDTH),
        ("--gds-sidebar-padding", SIDEBAR_PADDING),
        ("--gds-sidebar-section-padding", SIDEBAR_SECTION_PADDING),
        ("--gds-sidebar-item-height", SIDEBAR_ITEM_HEIGHT),
        ("--gds-content-padding", CONTENT_PADDING),
        ("--gds-terminal-content-inset", TERMINAL_CONTENT_INSET),
        ("--gds-icon-button-size", ICON_BUTTON_SIZE),
        ("--gds-terminal-header-height", TERMINAL_HEADER_HEIGHT),
        ("--gds-icon-size", ICON_SIZE),
        ("--gds-app-menu-icon-size", APPLICATION_MENU_ICON_SIZE),
        ("--gds-sidebar-icon-size", SIDEBAR_ICON_SIZE),
        ("--gds-small-icon-size", SMALL_ICON_SIZE),
        ("--gds-list-row-radius", DataListStyle::ROW_RADIUS),
        ("--gds-list-cell-padding-x", DataListStyle::CELL_PADDING_X),
        ("--gds-list-cell-padding-y", DataListStyle::CELL_PADDING_Y),
        ("--gds-scrollbar-lane", DataListStyle::SCROLLBAR_LANE),
    ];
    for (name, value) in fixed_pairs {
        let _ = writeln!(out, "    {name}: {value}px;");
    }

    let scaled_list_pairs = [
        (
            "--gds-list-row-height",
            DataListStyle::scaled_row_height(font_scale),
        ),
        (
            "--gds-list-selection-height",
            DataListStyle::scaled_selection_height(font_scale),
        ),
        (
            "--gds-list-header-height",
            DataListStyle::scaled_header_height(font_scale),
        ),
        (
            "--gds-list-selection-inset",
            DataListStyle::SELECTION_INSET * font_scale,
        ),
        (
            "--gds-list-name-size",
            DataListStyle::NAME_SIZE * font_scale,
        ),
        (
            "--gds-list-meta-size",
            DataListStyle::META_SIZE * font_scale,
        ),
        (
            "--gds-list-header-size",
            DataListStyle::HEADER_SIZE * font_scale,
        ),
    ];
    for (name, value) in scaled_list_pairs {
        let _ = writeln!(out, "    {name}: {value:.2}px;");
    }
}

#[cfg(test)]
mod tests;
