//! Terminal roles belong to the appearance snapshot, not the renderer.
use gpui_kit::{Hsla, rgb};

use super::Mode;

/// Opaque terminal colors; the content material supplies background opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalColors {
    pub foreground: Hsla,
    pub background: Hsla,
    pub cursor: Hsla,
    pub cursor_text: Hsla,
    pub selection: Hsla,
    pub selection_text: Hsla,
    pub normal: [Hsla; 8],
    pub bright: [Hsla; 8],
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
        .map(|value| rgb(value).into());
        let (foreground, background) = match mode {
            Mode::Light => (rgb(0x202020).into(), rgb(0xffffff).into()),
            Mode::Dark => (rgb(0xc5c8c6).into(), rgb(0x1d1f21).into()),
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
}
