//! Resolved GDS color roles shared across custom CSS properties and `zgui-ui-tokens`.
use std::fmt::Write as _;
use zgui::canvas::zgui_color::{Color, ColorSpace};
use zgui_ui_tokens::Theme;

use super::color::{
    ColorExt as _, color_css, contrast_ratio, ensure_contrast, relative_luminance, rgb,
};
use crate::appearance::{motion, typography};

const HOVER_OPACITY: f32 = 0.08;
const PRESSED_OPACITY: f32 = 0.12;
const DRAG_OPACITY: f32 = 0.12;
const CONTROL_BORDER_OUTLINE_MIX: f32 = 0.35;
const SWITCH_OFF_TINT_LIGHT: f32 = 0.14;
const SWITCH_OFF_TINT_DARK: f32 = 0.16;
const SWITCH_OFF_TINT_HIGH_CONTRAST: f32 = 0.28;
const SWITCH_THUMB_INK_LIGHT: f32 = 0.28;

// WCAG 2.1 contrast ratio floors applied under `Contrast::More` (XDG Portal `contrast = 1`).
const HC_PRIMARY_TEXT_RATIO: f32 = 7.0;
const HC_MUTED_TEXT_RATIO: f32 = 5.0;
const HC_BORDER_RATIO: f32 = 3.5;
const HC_OUTLINE_RATIO: f32 = 5.0;
const HC_ACCENT_RATIO: f32 = 4.5;
const HC_LINK_RATIO: f32 = 5.5;
const HC_SELECTION_SURFACE_RATIO: f32 = 1.35;

fn enhance_accent_pair(
    bg: Color,
    fg: Color,
    surface: Color,
    min_bg_vs_surface: f32,
    min_fg_vs_bg: f32,
) -> (Color, Color) {
    let mut bg = ensure_contrast(bg, surface, min_bg_vs_surface);
    let white = rgb(0xffffff);
    let black = rgb(0x000000);
    // Avoid the mid-luminance dead zone where neither pure black nor pure white ink can achieve
    // `min_fg_vs_bg`: push the accent further away from `surface` while preserving its hue.
    if contrast_ratio(white, bg).max(contrast_ratio(black, bg)) < min_fg_vs_bg {
        let ink = if relative_luminance(surface) >= 0.5 {
            white
        } else {
            black
        };
        bg = ensure_contrast(bg, ink, min_fg_vs_bg);
    }
    let fg = ensure_contrast(fg, bg, min_fg_vs_bg);
    (bg, fg)
}

fn contrasting_ink(fill: Color) -> Color {
    let [r, g, b] = fill.to_space(ColorSpace::Srgb).components();
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if luminance > 0.58 {
        rgb(0x1b1b1b)
    } else {
        rgb(0xffffff)
    }
}

/// Resolved GDS semantic roles and full Material 3 / DMS color tokens shared by native components
/// and custom surfaces. Surface/foreground and container/`on_*` pairs must be used together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub terminal: super::TerminalColors,

    // GDS semantic surface & boundary roles.
    pub content: Color,
    pub sidebar: Color,
    pub foreground: Color,
    pub sidebar_foreground: Color,
    pub muted_foreground: Color,
    pub surface: Color,
    pub surface_foreground: Color,
    /// Raised panels, dialogs, and menus.
    pub panel: Color,
    pub panel_foreground: Color,
    /// Neutral raised interactive controls (`ButtonVariant::Outline`, Cancel actions).
    pub control: Color,
    pub control_foreground: Color,
    /// Subtle separators; `outline` is the stronger control boundary.
    pub border: Color,
    pub outline: Color,
    pub shadow: Color,
    pub scrim: Color,

    // GDS semantic accent & feedback roles.
    pub primary: Color,
    pub primary_foreground: Color,
    pub selection: Color,
    pub selection_foreground: Color,
    pub tooltip: Color,
    pub tooltip_foreground: Color,
    pub link: Color,
    pub error: Color,
    pub error_foreground: Color,
    /// Business status roles absent from Material exports use suite defaults.
    pub success: Color,
    pub success_foreground: Color,
    pub warning: Color,
    pub warning_foreground: Color,

    // Complete Material 3 / DMS aligned palette tokens (`surface` maps to `content`).
    pub background: Color,
    pub on_background: Color,
    pub on_surface: Color,
    pub surface_dim: Color,
    pub surface_bright: Color,
    pub surface_container_lowest: Color,
    pub surface_container_low: Color,
    pub surface_container: Color,
    pub surface_container_high: Color,
    pub surface_container_highest: Color,
    pub surface_variant: Color,
    pub on_surface_variant: Color,
    pub surface_tint: Color,
    pub inverse_surface: Color,
    pub inverse_on_surface: Color,
    pub inverse_primary: Color,
    pub outline_variant: Color,
    pub source_color: Color,

    pub on_primary: Color,
    pub primary_container: Color,
    pub on_primary_container: Color,
    pub primary_fixed: Color,
    pub primary_fixed_dim: Color,
    pub on_primary_fixed: Color,
    pub on_primary_fixed_variant: Color,

    pub secondary: Color,
    pub on_secondary: Color,
    pub secondary_container: Color,
    pub on_secondary_container: Color,
    pub secondary_fixed: Color,
    pub secondary_fixed_dim: Color,
    pub on_secondary_fixed: Color,
    pub on_secondary_fixed_variant: Color,

    pub tertiary: Color,
    pub on_tertiary: Color,
    pub tertiary_container: Color,
    pub on_tertiary_container: Color,
    pub tertiary_fixed: Color,
    pub tertiary_fixed_dim: Color,
    pub on_tertiary_fixed: Color,
    pub on_tertiary_fixed_variant: Color,

    pub on_error: Color,
    pub error_container: Color,
    pub on_error_container: Color,
}

impl Palette {
    /// Populate Material 3 aligned roles and neutral `control` surfaces from this palette's
    /// resolved semantic roles when a theme source does not supply explicit Material tokens.
    pub(super) fn sync_material_defaults(&mut self, mode: super::Mode) {
        self.background = self.content;
        self.on_background = self.foreground;
        self.on_surface = self.foreground;
        self.surface_container_lowest = match mode {
            super::Mode::Light => self.content.overlay(rgb(0xffffff).with_alpha(0.75)),
            super::Mode::Dark => self.content.overlay(rgb(0x000000).with_alpha(0.28)),
        };
        self.surface_container_low = self.sidebar;
        self.surface_container = self.surface;
        self.surface_container_high = self.panel;
        self.surface_container_highest = match mode {
            super::Mode::Light => self.panel.overlay(self.foreground.with_alpha(0.04)),
            super::Mode::Dark => self.panel.overlay(self.foreground.with_alpha(0.07)),
        };
        self.surface_dim = match mode {
            super::Mode::Light => self.content.overlay(rgb(0x000000).with_alpha(0.08)),
            super::Mode::Dark => self.content,
        };
        self.surface_bright = match mode {
            super::Mode::Light => self.content,
            super::Mode::Dark => self.panel.overlay(self.foreground.with_alpha(0.12)),
        };
        self.surface_variant = self.surface.overlay(self.foreground.with_alpha(0.06));
        self.on_surface_variant = self.muted_foreground;
        self.surface_tint = self.primary;
        self.inverse_surface = self.foreground;
        self.inverse_on_surface = self.content;
        self.inverse_primary = self.selection;
        self.outline_variant = self.border;
        self.scrim = self.shadow;
        self.source_color = self.primary;

        self.on_primary = self.primary_foreground;
        self.primary_container = self.selection;
        self.on_primary_container = self.selection_foreground;
        self.primary_fixed = self.selection;
        self.primary_fixed_dim = self.primary;
        self.on_primary_fixed = self.selection_foreground;
        self.on_primary_fixed_variant = self.primary;

        let derived_secondary = self.muted_foreground.overlay(self.primary.with_alpha(0.35));
        self.secondary = derived_secondary;
        self.on_secondary = contrasting_ink(derived_secondary);
        self.secondary_container = self.surface.overlay(self.primary.with_alpha(0.12));
        self.on_secondary_container = self.surface_foreground;
        self.secondary_fixed = self.secondary_container;
        self.secondary_fixed_dim = self.secondary;
        self.on_secondary_fixed = self.on_secondary_container;
        self.on_secondary_fixed_variant = self.secondary;

        self.tertiary = self.link;
        self.on_tertiary = contrasting_ink(self.link);
        self.tertiary_container = self.surface.overlay(self.link.with_alpha(0.14));
        self.on_tertiary_container = self.surface_foreground;
        self.tertiary_fixed = self.tertiary_container;
        self.tertiary_fixed_dim = self.tertiary;
        self.on_tertiary_fixed = self.on_tertiary_container;
        self.on_tertiary_fixed_variant = self.tertiary;

        self.on_error = self.error_foreground;
        let error_wash = match mode {
            super::Mode::Light => 0.14,
            super::Mode::Dark => 0.24,
        };
        self.error_container = self.content.overlay(self.error.with_alpha(error_wash));
        self.on_error_container = self.foreground;

        self.control = match mode {
            super::Mode::Light => self.surface_container_lowest,
            super::Mode::Dark => self.surface_container_highest,
        };
        self.control_foreground = self.foreground;
    }

    /// Lift text, boundary, accent, and status contrast floors across any theme source while
    /// preserving each palette's hue identity (`org.freedesktop.appearance.contrast == 1`).
    pub(super) fn enhance_contrast(&mut self, mode: super::Mode) {
        let _ = mode;
        // 1. Primary text roles (WCAG AAA >= 7:1).
        self.foreground = ensure_contrast(self.foreground, self.content, HC_PRIMARY_TEXT_RATIO);
        self.sidebar_foreground =
            ensure_contrast(self.sidebar_foreground, self.sidebar, HC_PRIMARY_TEXT_RATIO);
        self.surface_foreground =
            ensure_contrast(self.surface_foreground, self.surface, HC_PRIMARY_TEXT_RATIO);
        self.panel_foreground =
            ensure_contrast(self.panel_foreground, self.panel, HC_PRIMARY_TEXT_RATIO);
        self.control_foreground =
            ensure_contrast(self.control_foreground, self.control, HC_PRIMARY_TEXT_RATIO);
        self.tooltip_foreground =
            ensure_contrast(self.tooltip_foreground, self.tooltip, HC_PRIMARY_TEXT_RATIO);

        // 2. Secondary / muted text (>= 5:1 across content, surface, and panel).
        self.muted_foreground =
            ensure_contrast(self.muted_foreground, self.content, HC_MUTED_TEXT_RATIO);
        self.muted_foreground =
            ensure_contrast(self.muted_foreground, self.surface, HC_MUTED_TEXT_RATIO);
        self.muted_foreground =
            ensure_contrast(self.muted_foreground, self.panel, HC_MUTED_TEXT_RATIO);

        // 3. Structural boundaries: crisp separators (>= 3.5:1) and control outlines (>= 5:1).
        self.border = ensure_contrast(self.border, self.content, HC_BORDER_RATIO);
        self.border = ensure_contrast(self.border, self.surface, HC_BORDER_RATIO);
        self.border = ensure_contrast(self.border, self.panel, HC_BORDER_RATIO);
        self.outline = ensure_contrast(self.outline, self.content, HC_OUTLINE_RATIO);
        self.outline = ensure_contrast(self.outline, self.surface, HC_OUTLINE_RATIO);
        self.outline = ensure_contrast(self.outline, self.control, HC_OUTLINE_RATIO);

        // 4. Accents, selection, links, and semantic status pairs.
        (self.primary, self.primary_foreground) = enhance_accent_pair(
            self.primary,
            self.primary_foreground,
            self.content,
            HC_ACCENT_RATIO,
            HC_PRIMARY_TEXT_RATIO,
        );
        self.selection = ensure_contrast(self.selection, self.content, HC_SELECTION_SURFACE_RATIO);
        self.selection_foreground = ensure_contrast(
            self.selection_foreground,
            self.selection,
            HC_PRIMARY_TEXT_RATIO,
        );
        self.link = ensure_contrast(self.link, self.content, HC_LINK_RATIO);
        (self.error, self.error_foreground) = enhance_accent_pair(
            self.error,
            self.error_foreground,
            self.content,
            HC_ACCENT_RATIO,
            HC_PRIMARY_TEXT_RATIO,
        );
        (self.success, self.success_foreground) = enhance_accent_pair(
            self.success,
            self.success_foreground,
            self.content,
            HC_ACCENT_RATIO,
            HC_PRIMARY_TEXT_RATIO,
        );
        (self.warning, self.warning_foreground) = enhance_accent_pair(
            self.warning,
            self.warning_foreground,
            self.content,
            HC_ACCENT_RATIO,
            HC_PRIMARY_TEXT_RATIO,
        );

        // 5. Keep Material 3 mirrors and terminal defaults synchronized with enhanced roles.
        self.on_background = self.foreground;
        self.on_surface = self.foreground;
        self.on_surface_variant = self.muted_foreground;
        self.outline_variant = self.border;
        self.on_primary = self.primary_foreground;
        self.primary_container = self.selection;
        self.on_primary_container = self.selection_foreground;
        self.on_secondary =
            ensure_contrast(self.on_secondary, self.secondary, HC_PRIMARY_TEXT_RATIO);
        self.on_secondary_container = ensure_contrast(
            self.on_secondary_container,
            self.secondary_container,
            HC_PRIMARY_TEXT_RATIO,
        );
        self.on_tertiary = ensure_contrast(self.on_tertiary, self.tertiary, HC_PRIMARY_TEXT_RATIO);
        self.on_tertiary_container = ensure_contrast(
            self.on_tertiary_container,
            self.tertiary_container,
            HC_PRIMARY_TEXT_RATIO,
        );
        self.on_error = self.error_foreground;
        self.on_error_container = ensure_contrast(
            self.on_error_container,
            self.error_container,
            HC_PRIMARY_TEXT_RATIO,
        );
        self.terminal.foreground = ensure_contrast(
            self.terminal.foreground,
            self.terminal.background,
            HC_PRIMARY_TEXT_RATIO,
        );
        self.terminal.cursor = ensure_contrast(
            self.terminal.cursor,
            self.terminal.background,
            HC_PRIMARY_TEXT_RATIO,
        );
        self.terminal.selection_text = ensure_contrast(
            self.terminal.selection_text,
            self.terminal.selection,
            HC_PRIMARY_TEXT_RATIO,
        );
    }

    /// Sources without terminal data share ANSI defaults and their own paired
    /// text/surface roles. Call after resolving the source's UI roles.
    pub(super) fn terminal_fallback(&mut self, mode: super::Mode) {
        self.terminal = super::TerminalColors {
            foreground: self.foreground,
            background: self.content,
            cursor: self.foreground,
            cursor_text: self.content,
            selection: self.selection,
            selection_text: self.selection_foreground,
            ..super::TerminalColors::fallback(mode)
        };
    }

    /// State layers keep neutral interaction distinct from persistent selection.
    pub fn hover(self) -> Color {
        self.foreground.with_alpha(HOVER_OPACITY)
    }

    pub fn pressed(self) -> Color {
        self.foreground.with_alpha(PRESSED_OPACITY)
    }

    /// Opaque hover fill for raised neutral controls (`--gds-control`).
    #[must_use]
    pub fn control_hover(self) -> Color {
        self.control
            .overlay(self.control_foreground.with_alpha(HOVER_OPACITY))
    }

    /// Opaque pressed fill for raised neutral controls (`--gds-control`).
    #[must_use]
    pub fn control_pressed(self) -> Color {
        self.control
            .overlay(self.control_foreground.with_alpha(PRESSED_OPACITY))
    }

    /// Balanced contour border for raised neutral controls across light, dark, and high-contrast themes.
    #[must_use]
    pub fn control_border(self) -> Color {
        self.border
            .overlay(self.outline.with_alpha(CONTROL_BORDER_OUTLINE_MIX))
    }

    pub fn sidebar_hover(self) -> Color {
        self.sidebar_foreground.with_alpha(HOVER_OPACITY)
    }

    pub fn drag_target(self) -> Color {
        self.primary.with_alpha(DRAG_OPACITY)
    }

    fn inferred_mode(self) -> super::Mode {
        if self.content.lightness() < 0.5 {
            super::Mode::Dark
        } else {
            super::Mode::Light
        }
    }

    /// Unchecked toggle track fill resolved for `mode`, keeping distinct contrast from `content`
    /// and pairing cleanly with `outline` borders and `switch_thumb_off_for(mode)` thumbs.
    #[must_use]
    pub fn switch_off_for(self, mode: super::Mode) -> Color {
        self.switch_off_with_contrast(mode, super::Contrast::Normal)
    }

    fn switch_off_with_contrast(self, mode: super::Mode, contrast: super::Contrast) -> Color {
        let tint = if contrast.is_more() {
            SWITCH_OFF_TINT_HIGH_CONTRAST
        } else {
            match mode {
                super::Mode::Light => SWITCH_OFF_TINT_LIGHT,
                super::Mode::Dark => SWITCH_OFF_TINT_DARK,
            }
        };
        self.panel.overlay(self.outline.with_alpha(tint))
    }

    fn control_border_for(self, contrast: super::Contrast) -> Color {
        if contrast.is_more() {
            self.outline
        } else {
            self.control_border()
        }
    }

    /// Unchecked toggle thumb fill resolved for `mode` (high-contrast neutral ink on light tracks,
    /// crisp foreground circle on dark tracks).
    #[must_use]
    pub fn switch_thumb_off_for(self, mode: super::Mode) -> Color {
        match mode {
            super::Mode::Light => self
                .outline
                .overlay(self.foreground.with_alpha(SWITCH_THUMB_INK_LIGHT)),
            super::Mode::Dark => self.foreground,
        }
    }

    #[must_use]
    pub fn switch_off(self) -> Color {
        self.switch_off_for(self.inferred_mode())
    }

    #[must_use]
    pub fn switch_thumb_off(self) -> Color {
        self.switch_thumb_off_for(self.inferred_mode())
    }

    pub(crate) fn write_css_declarations(self, contrast: super::Contrast, out: &mut String) {
        let mode = self.inferred_mode();
        let colors = [
            // GDS semantic & interactive tokens.
            ("--gds-content", self.content),
            ("--gds-sidebar", self.sidebar),
            ("--gds-foreground", self.foreground),
            ("--gds-sidebar-foreground", self.sidebar_foreground),
            ("--gds-muted-foreground", self.muted_foreground),
            ("--gds-surface", self.surface),
            ("--gds-surface-foreground", self.surface_foreground),
            ("--gds-panel", self.panel),
            ("--gds-panel-foreground", self.panel_foreground),
            ("--gds-control", self.control),
            ("--gds-control-foreground", self.control_foreground),
            ("--gds-control-hover", self.control_hover()),
            ("--gds-control-pressed", self.control_pressed()),
            ("--gds-control-border", self.control_border_for(contrast)),
            ("--gds-border", self.border),
            ("--gds-outline", self.outline),
            ("--gds-shadow", self.shadow),
            ("--gds-scrim", self.scrim),
            ("--gds-scrim-backdrop", self.scrim.with_alpha(0.5)),
            ("--gds-primary", self.primary),
            ("--gds-primary-foreground", self.primary_foreground),
            ("--gds-selection", self.selection),
            ("--gds-selection-foreground", self.selection_foreground),
            ("--gds-tooltip", self.tooltip),
            ("--gds-tooltip-foreground", self.tooltip_foreground),
            ("--gds-link", self.link),
            ("--gds-error", self.error),
            ("--gds-error-foreground", self.error_foreground),
            ("--gds-success", self.success),
            ("--gds-success-foreground", self.success_foreground),
            ("--gds-warning", self.warning),
            ("--gds-warning-foreground", self.warning_foreground),
            ("--gds-hover", self.hover()),
            ("--gds-pressed", self.pressed()),
            ("--gds-sidebar-hover", self.sidebar_hover()),
            ("--gds-drag-target", self.drag_target()),
            (
                "--gds-switch-off",
                self.switch_off_with_contrast(mode, contrast),
            ),
            ("--gds-switch-thumb-off", self.switch_thumb_off()),
            // Complete Material 3 / DMS aligned tokens.
            ("--gds-background", self.background),
            ("--gds-on-background", self.on_background),
            ("--gds-on-surface", self.on_surface),
            ("--gds-surface-dim", self.surface_dim),
            ("--gds-surface-bright", self.surface_bright),
            (
                "--gds-surface-container-lowest",
                self.surface_container_lowest,
            ),
            ("--gds-surface-container-low", self.surface_container_low),
            ("--gds-surface-container", self.surface_container),
            ("--gds-surface-container-high", self.surface_container_high),
            (
                "--gds-surface-container-highest",
                self.surface_container_highest,
            ),
            ("--gds-surface-variant", self.surface_variant),
            ("--gds-on-surface-variant", self.on_surface_variant),
            ("--gds-surface-tint", self.surface_tint),
            ("--gds-inverse-surface", self.inverse_surface),
            ("--gds-inverse-on-surface", self.inverse_on_surface),
            ("--gds-inverse-primary", self.inverse_primary),
            ("--gds-outline-variant", self.outline_variant),
            ("--gds-source-color", self.source_color),
            ("--gds-on-primary", self.on_primary),
            ("--gds-primary-container", self.primary_container),
            ("--gds-on-primary-container", self.on_primary_container),
            ("--gds-primary-fixed", self.primary_fixed),
            ("--gds-primary-fixed-dim", self.primary_fixed_dim),
            ("--gds-on-primary-fixed", self.on_primary_fixed),
            (
                "--gds-on-primary-fixed-variant",
                self.on_primary_fixed_variant,
            ),
            ("--gds-secondary", self.secondary),
            ("--gds-on-secondary", self.on_secondary),
            ("--gds-secondary-container", self.secondary_container),
            ("--gds-on-secondary-container", self.on_secondary_container),
            ("--gds-secondary-fixed", self.secondary_fixed),
            ("--gds-secondary-fixed-dim", self.secondary_fixed_dim),
            ("--gds-on-secondary-fixed", self.on_secondary_fixed),
            (
                "--gds-on-secondary-fixed-variant",
                self.on_secondary_fixed_variant,
            ),
            ("--gds-tertiary", self.tertiary),
            ("--gds-on-tertiary", self.on_tertiary),
            ("--gds-tertiary-container", self.tertiary_container),
            ("--gds-on-tertiary-container", self.on_tertiary_container),
            ("--gds-tertiary-fixed", self.tertiary_fixed),
            ("--gds-tertiary-fixed-dim", self.tertiary_fixed_dim),
            ("--gds-on-tertiary-fixed", self.on_tertiary_fixed),
            (
                "--gds-on-tertiary-fixed-variant",
                self.on_tertiary_fixed_variant,
            ),
            ("--gds-on-error", self.on_error),
            ("--gds-error-container", self.error_container),
            ("--gds-on-error-container", self.on_error_container),
        ];
        for (name, value) in colors {
            let _ = writeln!(out, "    {name}: {};", color_css(value));
        }
        self.terminal.write_css_declarations(out);
    }

    /// Lower this resolved GDS palette and appearance preferences into `zgui-ui-tokens::Theme`.
    pub fn tokens_theme(
        self,
        mode: super::Mode,
        typography: &typography::Resolved,
        motion: motion::Policy,
        rounded_corners: bool,
        contrast: super::Contrast,
    ) -> Theme {
        let mut theme = match mode {
            super::Mode::Light => Theme::light(),
            super::Mode::Dark => Theme::dark(),
        };
        theme.color.background = color_css(self.content);
        theme.color.foreground = color_css(self.foreground);
        theme.color.card = color_css(self.surface);
        theme.color.card_foreground = color_css(self.surface_foreground);
        theme.color.popover = color_css(self.panel);
        theme.color.popover_foreground = color_css(self.panel_foreground);
        theme.color.tooltip = color_css(self.tooltip);
        theme.color.tooltip_foreground = color_css(self.tooltip_foreground);
        theme.color.tooltip_border = color_css(self.border);
        theme.color.primary = color_css(self.primary);
        theme.color.primary_foreground = color_css(self.primary_foreground);
        theme.color.secondary = color_css(self.secondary_container);
        theme.color.secondary_foreground = color_css(self.on_secondary_container);
        theme.color.muted = color_css(self.surface);
        theme.color.muted_foreground = color_css(self.muted_foreground);
        theme.color.accent = color_css(self.selection);
        theme.color.accent_foreground = color_css(self.selection_foreground);
        theme.color.destructive = color_css(self.error);
        theme.color.destructive_foreground = color_css(self.error_foreground);
        theme.color.danger = "var(--zui-color-destructive)".to_owned();
        theme.color.success = color_css(self.success);
        theme.color.success_foreground = color_css(self.success_foreground);
        theme.color.warning = color_css(self.warning);
        theme.color.warning_foreground = color_css(self.warning_foreground);
        theme.color.info = color_css(self.primary);
        theme.color.info_foreground = color_css(self.primary_foreground);
        theme.color.border = color_css(self.border);
        theme.color.input = color_css(self.outline);
        theme.color.ring = color_css(self.primary);
        theme.color.scrim = color_css(self.scrim.with_alpha(0.5));
        theme.color.sidebar = color_css(self.sidebar);
        theme.color.sidebar_foreground = color_css(self.sidebar_foreground);
        theme.color.sidebar_primary = color_css(self.primary);
        theme.color.sidebar_primary_foreground = color_css(self.primary_foreground);
        theme.color.sidebar_accent = color_css(self.selection);
        theme.color.sidebar_accent_foreground = color_css(self.selection_foreground);
        theme.color.sidebar_border = color_css(self.border);
        theme.color.sidebar_ring = color_css(self.primary);

        theme.control.ghost_hover = color_css(self.hover());
        theme.control.outline_border = color_css(self.control_border_for(contrast));
        theme.control.outline_hover = color_css(self.control_hover());
        theme.control.switch_off = color_css(self.switch_off_with_contrast(mode, contrast));
        theme.control.switch_thumb = color_css(self.switch_thumb_off_for(mode));
        theme.control.switch_thumb_on = color_css(self.primary_foreground);

        let _ = rounded_corners;
        "4px".clone_into(&mut theme.radius.sm);
        "6px".clone_into(&mut theme.radius.md);
        "8px".clone_into(&mut theme.radius.lg);
        "10px".clone_into(&mut theme.radius.xl);
        "8px".clone_into(&mut theme.radius.base);

        theme.focus.disable_ring_shadows();
        if contrast.is_more() {
            "2px solid var(--zui-color-ring)".clone_into(&mut theme.focus.ring_outline);
            "-2px".clone_into(&mut theme.focus.ring_offset);
        } else {
            "1px solid var(--zui-color-ring)".clone_into(&mut theme.focus.ring_outline);
            "-1px".clone_into(&mut theme.focus.ring_offset);
        }

        let scale = typography.scale();
        theme.typography.family_sans = typography.sans_css_stack();
        theme.typography.family_mono = typography.mono_css_stack();
        theme.typography.size_xs = format!("{:.2}px", 12.0 * scale);
        theme.typography.size_sm = format!("{:.2}px", 14.0 * scale);
        theme.typography.size_md = format!("{:.2}px", 16.0 * scale);
        theme.typography.size_lg = format!("{:.2}px", 16.0 * scale);
        theme.typography.size_xl = format!("{:.2}px", 20.0 * scale);
        theme.typography.size_x2l = format!("{:.2}px", 20.0 * scale);
        theme.typography.size_x3l = format!("{:.2}px", 20.0 * scale);
        theme.typography.leading_xs = format!("{:.2}px", 16.0 * scale);
        theme.typography.leading_sm = format!("{:.2}px", 20.0 * scale);
        theme.typography.leading_md = format!("{:.2}px", 22.0 * scale);
        theme.typography.leading_lg = format!("{:.2}px", 22.0 * scale);
        theme.typography.leading_xl = format!("{:.2}px", 26.0 * scale);
        theme.typography.leading_x2l = format!("{:.2}px", 26.0 * scale);
        theme.typography.leading_x3l = format!("{:.2}px", 26.0 * scale);

        if motion.reduced() {
            "0ms".clone_into(&mut theme.motion.duration_fast);
            "0ms".clone_into(&mut theme.motion.duration_normal);
            "0ms".clone_into(&mut theme.motion.duration_slow);
            "0ms".clone_into(&mut theme.motion.duration_slower);
            "0ms".clone_into(&mut theme.motion.duration_slowest);
        } else {
            theme.motion.duration_normal = format!("{}ms", motion::WORKSPACE_DURATION_MS);
            theme.motion.ease_out = motion::WORKSPACE_EASING.to_owned();
        }
        theme
    }
}

#[cfg(test)]
mod tests {
    use crate::appearance::theme::{Appearance, BuiltinTheme, Mode, ModePreference, Source};
    use crate::appearance::{motion, typography};

    #[test]
    fn switch_and_control_palette_tokens_are_emitted_and_contrast_across_all_builtin_themes() {
        let typo = typography::Preferences::default()
            .resolve()
            .expect("default typography");
        let motion_policy = motion::Policy::default();

        for builtin in BuiltinTheme::ALL {
            for (pref, expected_mode) in [
                (ModePreference::Light, Mode::Light),
                (ModePreference::Dark, Mode::Dark),
            ] {
                let prepared = Appearance {
                    source: Source::Builtin,
                    builtin,
                    mode: pref,
                    ..Appearance::default()
                }
                .prepare(expected_mode)
                .expect("prepare builtin theme");

                let palette = prepared.colors();
                assert_ne!(
                    palette.switch_off_for(expected_mode),
                    palette.switch_thumb_off_for(expected_mode),
                    "unchecked switch track and thumb must contrast in {builtin:?} {expected_mode:?}"
                );
                assert_ne!(
                    palette.primary, palette.primary_foreground,
                    "checked switch track and thumb must contrast in {builtin:?} {expected_mode:?}"
                );
                assert_ne!(
                    palette.control, palette.control_foreground,
                    "neutral control fill and ink must contrast in {builtin:?} {expected_mode:?}"
                );
                assert_ne!(
                    palette.control, palette.panel,
                    "neutral control fill must stand apart from dialog panel in {builtin:?} {expected_mode:?}"
                );

                let css = prepared.gds_stylesheet_css(&typo, motion_policy);
                for token in [
                    "--gds-control:",
                    "--gds-control-foreground:",
                    "--gds-control-hover:",
                    "--gds-control-pressed:",
                    "--gds-control-border:",
                    "--gds-surface-container-lowest:",
                    "--gds-surface-container-highest:",
                    "--gds-secondary:",
                    "--gds-secondary-container:",
                    "--gds-tertiary:",
                    "--gds-tertiary-container:",
                    "--gds-error-container:",
                    "--gds-inverse-surface:",
                    "--gds-switch-off:",
                    "--gds-switch-thumb-off:",
                ] {
                    assert!(css.contains(token), "missing {token} in {builtin:?}");
                }

                let tokens = prepared.tokens_theme(&typo, motion_policy);
                assert_ne!(tokens.control.switch_off, tokens.control.switch_thumb);
                assert!(!tokens.control.switch_thumb_on.is_empty());
            }
        }
    }
}
