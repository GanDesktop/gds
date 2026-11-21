//! Terminal roles belong to the appearance snapshot, not the renderer.
use std::fmt::Write as _;
use zgui::canvas::zgui_color::Color;

use super::{
    Mode,
    provider::color::{color_css, rgb},
};

/// Opaque terminal colors; the content material supplies background opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalColors {
    pub foreground: Color,
    pub background: Color,
    pub cursor: Color,
    pub cursor_text: Color,
    pub selection: Color,
    pub selection_text: Color,
    pub normal: [Color; 8],
    pub bright: [Color; 8],
}

impl TerminalColors {
    /// Central fallback for sources without ANSI data. Chromatic roles are
    /// independent of UI status colors; light variants use darker ink.
    pub(super) fn fallback(mode: Mode) -> Self {
        const DARK: [u32; 16] = [
            0x1d1f21, 0xcc6666, 0xb5bd68, 0xf0c674, 0x81a2be, 0xb294bb, 0x8abeb7, 0xc5c8c6,
            0x666666, 0xd54e53, 0xb9ca4a, 0xe7c547, 0x7aa6da, 0xc397d8, 0x70c0b1, 0xeaeaea,
        ];
        const LIGHT: [u32; 16] = [
            0x202020, 0xa02020, 0x356b19, 0x806000, 0x204a87, 0x75507b, 0x006b70, 0x808080,
            0x505050, 0xb52a2a, 0x447a20, 0x926b00, 0x3060a0, 0x885a90, 0x007e85, 0xa0a0a0,
        ];
        let ansi = match mode {
            Mode::Light => LIGHT,
            Mode::Dark => DARK,
        }
        .map(rgb);
        let (foreground, background) = match mode {
            Mode::Light => (rgb(0x202020), rgb(0xffffff)),
            Mode::Dark => (rgb(0xc5c8c6), rgb(0x1d1f21)),
        };
        Self {
            foreground,
            background,
            cursor: foreground,
            cursor_text: background,
            selection: foreground,
            selection_text: background,
            normal: std::array::from_fn(|i| ansi[i]),
            bright: std::array::from_fn(|i| ansi[i + 8]),
        }
    }

    pub(crate) fn write_css_declarations(&self, out: &mut String) {
        let _ = writeln!(out, "    --gds-term-fg: {};", color_css(self.foreground));
        let _ = writeln!(out, "    --gds-term-bg: {};", color_css(self.background));
        let _ = writeln!(out, "    --gds-term-cursor: {};", color_css(self.cursor));
        let _ = writeln!(
            out,
            "    --gds-term-cursor-text: {};",
            color_css(self.cursor_text)
        );
        let _ = writeln!(
            out,
            "    --gds-term-selection: {};",
            color_css(self.selection)
        );
        let _ = writeln!(
            out,
            "    --gds-term-selection-text: {};",
            color_css(self.selection_text)
        );
        for (i, color) in self.normal.iter().chain(self.bright.iter()).enumerate() {
            let _ = writeln!(out, "    --gds-term-ansi-{i}: {};", color_css(*color));
        }
    }
}
