//! Terminal color semantics shared by painting and OSC color-query replies.

#[cfg(test)]
mod tests;

use alacritty_terminal::{
    term::{
        cell::Flags,
        color::{COUNT, Colors},
    },
    vte::ansi::{Color, NamedColor, Rgb},
};
use zgui::canvas::zgui_color::Color as ZguiColor;

use crate::{app::Gds, appearance::theme};

#[derive(Clone, Copy)]
pub(in crate::ui::terminal) struct Palette {
    pub background: ZguiColor,
    pub foreground: ZguiColor,
    pub selection: ZguiColor,
    pub selected_text: ZguiColor,
    pub cursor_text: ZguiColor,
    colors: [Rgb; COUNT],
}

impl Palette {
    pub fn current() -> Self {
        if let Some(gds) = Gds::try_current() {
            let theme = gds.appearance().theme();
            let roles = theme.colors().terminal;
            let alpha = theme.content_background().alpha();
            Self::from_roles(roles, alpha)
        } else {
            Self::fallback()
        }
    }

    pub(in crate::ui::terminal) fn fallback() -> Self {
        let mut colors = [Rgb { r: 0, g: 0, b: 0 }; COUNT];
        let defaults: [(u8, u8, u8); 16] = [
            (0x1d, 0x1f, 0x21),
            (0xcc, 0x66, 0x66),
            (0xb5, 0xbd, 0x68),
            (0xf0, 0xc6, 0x74),
            (0x81, 0xa2, 0xbe),
            (0xb2, 0x94, 0xbb),
            (0x8a, 0xbe, 0xb7),
            (0xc5, 0xc8, 0xc6),
            (0x66, 0x66, 0x66),
            (0xd5, 0x4e, 0x53),
            (0xb9, 0xca, 0x4a),
            (0xe7, 0xc5, 0x47),
            (0x7a, 0xa6, 0xda),
            (0xc3, 0x97, 0xd8),
            (0x70, 0xc0, 0xb1),
            (0xea, 0xea, 0xea),
        ];
        for (i, (r, g, b)) in defaults.iter().copied().enumerate() {
            let rgb = Rgb { r, g, b };
            colors[i] = rgb;
            if i < 8 {
                colors[NamedColor::DimBlack as usize + i] = dim(rgb);
            }
        }
        let fg = Rgb {
            r: 0xc5,
            g: 0xc8,
            b: 0xc6,
        };
        let bg = Rgb {
            r: 0x1d,
            g: 0x1f,
            b: 0x21,
        };
        colors[NamedColor::Foreground as usize] = fg;
        colors[NamedColor::Background as usize] = bg;
        colors[NamedColor::Cursor as usize] = fg;
        colors[NamedColor::BrightForeground as usize] = Rgb {
            r: 0xea,
            g: 0xea,
            b: 0xea,
        };
        colors[NamedColor::DimForeground as usize] = dim(fg);
        Self {
            background: from_rgb(bg),
            foreground: from_rgb(fg),
            selection: from_rgb(Rgb {
                r: 0x37,
                g: 0x3b,
                b: 0x41,
            }),
            selected_text: from_rgb(fg),
            cursor_text: from_rgb(bg),
            colors,
        }
    }

    pub(crate) fn from_roles(roles: theme::TerminalColors, opacity: f32) -> Self {
        let background = roles.background.with_alpha(opacity);
        let mut colors = [Rgb { r: 0, g: 0, b: 0 }; COUNT];
        for i in 0..8 {
            colors[i] = to_rgb(roles.normal[i]);
            colors[i + 8] = to_rgb(roles.bright[i]);
            colors[NamedColor::DimBlack as usize + i] = dim(colors[i]);
        }
        for (i, color) in colors.iter_mut().enumerate().take(232).skip(16) {
            let n = i - 16;
            let level = |v: usize| if v == 0 { 0 } else { (55 + 40 * v) as u8 };
            *color = Rgb {
                r: level(n / 36),
                g: level(n / 6 % 6),
                b: level(n % 6),
            };
        }
        for (i, color) in colors.iter_mut().enumerate().take(256).skip(232) {
            let v = (8 + (i - 232) * 10) as u8;
            *color = Rgb { r: v, g: v, b: v };
        }
        colors[NamedColor::Foreground as usize] = to_rgb(roles.foreground);
        colors[NamedColor::Background as usize] = to_rgb(background);
        colors[NamedColor::Cursor as usize] = to_rgb(roles.cursor);
        colors[NamedColor::BrightForeground as usize] = to_rgb(roles.foreground);
        colors[NamedColor::DimForeground as usize] = dim(to_rgb(roles.foreground));
        Self {
            background,
            foreground: opaque(roles.foreground),
            selection: opaque(roles.selection),
            selected_text: opaque(roles.selection_text),
            cursor_text: opaque(roles.cursor_text),
            colors,
        }
    }

    pub fn query_colors(&self) -> [Rgb; COUNT] {
        self.colors
    }

    pub fn resolve(&self, color: Color, overrides: &Colors) -> ZguiColor {
        match color {
            Color::Spec(value) => from_rgb(value),
            Color::Named(index) => {
                from_rgb(overrides[index].unwrap_or(self.colors[index as usize]))
            }
            Color::Indexed(index) => {
                from_rgb(overrides[index as usize].unwrap_or(self.colors[index as usize]))
            }
        }
    }

    pub fn foreground(&self, color: Color, flags: Flags, overrides: &Colors) -> ZguiColor {
        let resolved = self.resolve(color, overrides);
        if flags.contains(Flags::DIM) {
            from_rgb(dim(to_rgb(resolved)))
        } else {
            resolved
        }
    }

    pub fn background(&self, overrides: &Colors) -> ZguiColor {
        self.resolve(Color::Named(NamedColor::Background), overrides)
            .with_alpha(self.background.alpha())
    }
}

fn opaque(color: ZguiColor) -> ZguiColor {
    color.with_alpha(1.0)
}

pub(crate) fn to_rgb(color: ZguiColor) -> Rgb {
    let [r, g, b] = color.components();
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    Rgb {
        r: channel(r),
        g: channel(g),
        b: channel(b),
    }
}

fn from_rgb(color: Rgb) -> ZguiColor {
    ZguiColor::srgb(
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
        1.0,
    )
}

fn dim(color: Rgb) -> Rgb {
    let channel = |value: u8| (f32::from(value) * 0.66).round() as u8;
    Rgb {
        r: channel(color.r),
        g: channel(color.g),
        b: channel(color.b),
    }
}
