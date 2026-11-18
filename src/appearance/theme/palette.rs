use anti_webui::prelude::{Color, Theme};

/// Resolved GDS color roles shared by native components and custom surfaces.
/// Surface/foreground and action/foreground pairs must be used together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub terminal: super::TerminalColors,
    pub content: Color,
    pub sidebar: Color,
    pub foreground: Color,
    pub sidebar_foreground: Color,
    pub muted_foreground: Color,
    pub surface: Color,
    pub surface_foreground: Color,
    /// Raised panels, including the Agents control panel.
    pub panel: Color,
    pub panel_foreground: Color,
    /// Subtle separators; `outline` is the stronger control boundary.
    pub border: Color,
    pub outline: Color,
    pub shadow: Color,
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
}

impl Palette {
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
    pub fn hover(self) -> Color {
        self.foreground.with_alpha(0.08)
    }
    pub fn pressed(self) -> Color {
        self.foreground.with_alpha(0.12)
    }
    pub fn sidebar_hover(self) -> Color {
        self.sidebar_foreground.with_alpha(0.08)
    }
    pub fn drag_target(self) -> Color {
        self.primary.with_alpha(0.12)
    }
    pub(crate) fn widgets(self, mode: super::Mode) -> Theme {
        let mut theme = match mode {
            super::Mode::Light => Theme::light(),
            super::Mode::Dark => Theme::dark(),
        };
        theme.page = self.content;
        theme.surface = self.surface;
        theme.surface_muted = self.sidebar;
        theme.overlay = self.panel;
        theme.hover = self.hover();
        theme.pressed = self.pressed();
        theme.selected = self.selection;
        theme.text = self.foreground;
        theme.text_muted = self.muted_foreground;
        theme.text_disabled = self.muted_foreground.with_alpha(0.5);
        theme.on_accent = self.primary_foreground;
        theme.border = self.border;
        theme.border_strong = self.outline;
        theme.accent = self.primary;
        theme.accent_hover = self.primary.mix(self.primary_foreground, 0.08);
        theme.accent_pressed = self.primary.mix(self.primary_foreground, 0.12);
        theme.accent_text = self.link;
        theme.accent_muted = self.selection;
        theme.success = self.success;
        theme.success_muted = self.success.with_alpha(0.12);
        theme.success_text = self.success;
        theme.warning = self.warning;
        theme.warning_muted = self.warning.with_alpha(0.12);
        theme.warning_text = self.warning;
        theme.danger = self.error;
        theme.danger_hover = self.error.mix(self.error_foreground, 0.08);
        theme.danger_pressed = self.error.mix(self.error_foreground, 0.12);
        theme.danger_muted = self.error.with_alpha(0.12);
        theme.danger_text = self.error;
        theme.focus_ring = self.primary;
        theme.field_focus_ring = self.primary.with_alpha(0.25);
        theme.tooltip = self.tooltip;
        theme.on_tooltip = self.tooltip_foreground;
        theme.scrim = self.shadow.with_alpha(0.5);
        theme.text_selection = self.selection;
        theme.scrollbar = self.foreground.with_alpha(0.35);
        theme
    }
}
