use gpui_kit::component::ThemeColor;
use gpui_kit::{App, Global, Hsla, component::ActiveTheme, rgb};

const HOVER_OPACITY: f32 = 0.08;
const PRESSED_OPACITY: f32 = 0.12;
const DRAG_OPACITY: f32 = 0.12;
const SCROLL_THUMB_OPACITY: f32 = 0.35;
const SCROLL_THUMB_HOVER_OPACITY: f32 = 0.55;
/// Resolved GDS color roles shared by native components and custom surfaces.
/// Surface/foreground and action/foreground pairs must be used together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub terminal: super::TerminalColors,
    pub content: Hsla,
    pub sidebar: Hsla,
    pub foreground: Hsla,
    pub sidebar_foreground: Hsla,
    pub muted_foreground: Hsla,
    pub surface: Hsla,
    pub surface_foreground: Hsla,
    /// Raised panels, including the Agents control panel.
    pub panel: Hsla,
    pub panel_foreground: Hsla,
    /// Subtle separators; `outline` is the stronger control boundary.
    pub border: Hsla,
    pub outline: Hsla,
    pub shadow: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub selection: Hsla,
    pub selection_foreground: Hsla,
    pub tooltip: Hsla,
    pub tooltip_foreground: Hsla,
    pub link: Hsla,
    pub error: Hsla,
    pub error_foreground: Hsla,
    /// Business status roles absent from Material exports use suite defaults.
    pub success: Hsla,
    pub success_foreground: Hsla,
    pub warning: Hsla,
    pub warning_foreground: Hsla,
}

impl Global for Palette {}

impl Palette {
    /// Read the same snapshot used to populate GPUI Kit's theme.
    pub(crate) fn current(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_else(|| {
            let colors = cx.theme();
            let mode = if colors.background.l < 0.5 {
                super::Mode::Dark
            } else {
                super::Mode::Light
            };
            let mut palette = Self {
                terminal: super::TerminalColors::fallback(mode),
                content: colors.background,
                sidebar: colors.sidebar,
                foreground: colors.foreground,
                sidebar_foreground: colors.sidebar_foreground,
                muted_foreground: colors.muted_foreground,
                surface: colors.secondary,
                surface_foreground: colors.secondary_foreground,
                panel: colors.popover,
                panel_foreground: colors.popover_foreground,
                border: colors.border,
                outline: colors.input,
                shadow: rgb(0x000000).into(),
                primary: colors.primary,
                primary_foreground: colors.primary_foreground,
                selection: colors.accent,
                selection_foreground: colors.accent_foreground,
                tooltip: colors.popover,
                tooltip_foreground: colors.popover_foreground,
                link: colors.link,
                error: colors.danger,
                error_foreground: colors.danger_foreground,
                success: colors.success,
                success_foreground: colors.success_foreground,
                warning: colors.warning,
                warning_foreground: colors.warning_foreground,
            };
            palette.terminal_fallback(mode);
            palette
        })
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
    pub fn hover(self) -> Hsla {
        self.foreground.opacity(HOVER_OPACITY)
    }

    pub fn pressed(self) -> Hsla {
        self.foreground.opacity(PRESSED_OPACITY)
    }

    pub fn sidebar_hover(self) -> Hsla {
        self.sidebar_foreground.opacity(HOVER_OPACITY)
    }

    pub fn drag_target(self) -> Hsla {
        self.primary.opacity(DRAG_OPACITY)
    }

    pub(super) fn apply_to(self, colors: &mut ThemeColor) {
        let control_hover = self
            .surface
            .blend(self.surface_foreground.opacity(HOVER_OPACITY));
        let control_pressed = self
            .surface
            .blend(self.surface_foreground.opacity(PRESSED_OPACITY));
        let primary_hover = self
            .primary
            .blend(self.primary_foreground.opacity(HOVER_OPACITY));
        let primary_pressed = self
            .primary
            .blend(self.primary_foreground.opacity(PRESSED_OPACITY));
        colors.background = self.content;
        colors.foreground = self.foreground;
        colors.border = self.border;
        colors.input = self.outline;
        colors.primary = self.primary;
        colors.primary_foreground = self.primary_foreground;
        colors.primary_hover = primary_hover;
        colors.primary_active = primary_pressed;
        colors.ring = self.primary;
        colors.caret = self.primary;
        colors.selection = self.selection;
        colors.accent = self.selection;
        colors.accent_foreground = self.selection_foreground;
        colors.muted = self.surface;
        colors.muted_foreground = self.muted_foreground;
        colors.secondary = self.surface;
        colors.secondary_foreground = self.surface_foreground;
        colors.secondary_hover = control_hover;
        colors.secondary_active = control_pressed;
        colors.button = self.surface;
        colors.button_foreground = self.surface_foreground;
        colors.button_hover = control_hover;
        colors.button_active = control_pressed;
        colors.button_primary = self.primary;
        colors.button_primary_foreground = self.primary_foreground;
        colors.button_primary_hover = primary_hover;
        colors.button_primary_active = primary_pressed;
        colors.button_secondary = self.surface;
        colors.button_secondary_foreground = self.surface_foreground;
        colors.button_secondary_hover = control_hover;
        colors.button_secondary_active = control_pressed;
        colors.popover = self.tooltip;
        colors.popover_foreground = self.tooltip_foreground;
        colors.group_box = self.surface;
        colors.group_box_foreground = self.surface_foreground;
        colors.accordion = self.surface;
        colors.sidebar = self.sidebar;
        colors.sidebar_foreground = self.sidebar_foreground;
        colors.sidebar_border = self.border;
        colors.sidebar_accent = self.selection;
        colors.sidebar_accent_foreground = self.selection_foreground;
        colors.sidebar_primary = self.primary;
        colors.sidebar_primary_foreground = self.primary_foreground;
        colors.list = self.content;
        colors.list_even = self.content;
        colors.list_head = self.surface;
        colors.list_hover = self.hover();
        colors.list_active = self.selection;
        colors.list_active_border = self.primary;
        colors.table = self.content;
        colors.table_even = self.content;
        colors.table_head = self.surface;
        colors.table_head_foreground = self.muted_foreground;
        colors.table_hover = self.hover();
        colors.table_active = self.selection;
        colors.table_active_border = self.primary;
        colors.table_row_border = self.border.opacity(0.0);
        colors.table_foot = self.surface;
        colors.table_foot_foreground = self.muted_foreground;
        colors.tab = self.sidebar;
        colors.tab_foreground = self.sidebar_foreground;
        colors.tab_active = self.selection;
        colors.tab_active_foreground = self.selection_foreground;
        colors.tab_bar = self.sidebar;
        colors.tab_bar_segmented = self.surface;
        colors.title_bar = self.sidebar;
        colors.title_bar_border = self.border;
        colors.status_bar = self.sidebar;
        colors.status_bar_border = self.border;
        colors.link = self.link;
        colors.link_hover = self.link;
        colors.link_active = self.link;
        colors.drag_border = self.primary;
        colors.drop_target = self.drag_target();
        colors.progress_bar = self.primary;
        colors.slider_bar = self.primary;
        colors.slider_thumb = self.primary;
        colors.switch = self.outline;
        colors.switch_thumb = self.surface;
        colors.description_list_label = self.surface;
        colors.description_list_label_foreground = self.muted_foreground;
        colors.skeleton = self.surface;
        colors.window_border = self.border;
        colors.scrollbar = self.content.opacity(0.0);
        colors.scrollbar_thumb = self.foreground.opacity(SCROLL_THUMB_OPACITY);
        colors.scrollbar_thumb_hover = self.foreground.opacity(SCROLL_THUMB_HOVER_OPACITY);
        colors.danger = self.error;
        colors.danger_foreground = self.error_foreground;
        let danger_hover = self
            .error
            .blend(self.error_foreground.opacity(HOVER_OPACITY));
        let danger_active = self
            .error
            .blend(self.error_foreground.opacity(PRESSED_OPACITY));
        colors.danger_hover = danger_hover;
        colors.danger_active = danger_active;
        colors.button_danger = self.error;
        colors.button_danger_foreground = self.error_foreground;
        colors.button_danger_hover = danger_hover;
        colors.button_danger_active = danger_active;
        colors.info = self.primary;
        colors.info_foreground = self.primary_foreground;
        colors.info_hover = primary_hover;
        colors.info_active = primary_pressed;
        colors.button_info = self.primary;
        colors.button_info_foreground = self.primary_foreground;
        colors.button_info_hover = primary_hover;
        colors.button_info_active = primary_pressed;
        colors.success = self.success;
        colors.success_foreground = self.success_foreground;
        colors.success_hover = self
            .success
            .blend(self.success_foreground.opacity(HOVER_OPACITY));
        colors.success_active = self
            .success
            .blend(self.success_foreground.opacity(PRESSED_OPACITY));
        colors.button_success = colors.success;
        colors.button_success_foreground = colors.success_foreground;
        colors.button_success_hover = colors.success_hover;
        colors.button_success_active = colors.success_active;
        colors.warning = self.warning;
        colors.warning_foreground = self.warning_foreground;
        colors.warning_hover = self
            .warning
            .blend(self.warning_foreground.opacity(HOVER_OPACITY));
        colors.warning_active = self
            .warning
            .blend(self.warning_foreground.opacity(PRESSED_OPACITY));
        colors.button_warning = colors.warning;
        colors.button_warning_foreground = colors.warning_foreground;
        colors.button_warning_hover = colors.warning_hover;
        colors.button_warning_active = colors.warning_active;
    }
}
