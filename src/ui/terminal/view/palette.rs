//! Terminal color semantics, shared by painting and OSC color-query replies.
use crate::appearance::theme;
use alacritty_terminal::{
    term::{
        cell::Flags,
        color::{COUNT, Colors},
    },
    vte::ansi::{Color, NamedColor, Rgb},
};
use gpui_kit::{App, Hsla, rgb};

pub(in crate::ui::terminal) struct Palette {
    pub background: Hsla,
    pub foreground: Hsla,
    pub selection: Hsla,
    pub selected_text: Hsla,
    colors: [Rgb; COUNT],
}

impl Palette {
    pub fn new(cx: &App) -> Self {
        let theme = theme::Colors::current(cx);
        let background = theme::content_background(cx);
        let dark = theme.content.l < 0.5;
        let tone = |hue: f32| Hsla {
            h: hue,
            s: 0.6,
            l: if dark { 0.68 } else { 0.38 },
            a: 1.,
        };
        let normal = [
            rgb(0x1d1f21).into(),
            theme.error,
            theme.success,
            theme.warning,
            tone(0.61),
            tone(0.82),
            tone(0.5),
            theme.foreground,
        ];
        let mut colors = [Rgb { r: 0, g: 0, b: 0 }; COUNT];
        for i in 0..8 {
            colors[i] = to_rgb(normal[i]);
            colors[i + 8] = if i == 0 {
                to_rgb(theme.muted_foreground)
            } else {
                to_rgb(mix(normal[i], theme.foreground, 0.22))
            };
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
        colors[NamedColor::Foreground as usize] = to_rgb(theme.foreground);
        colors[NamedColor::Background as usize] = to_rgb(background);
        colors[NamedColor::Cursor as usize] = to_rgb(theme.foreground);
        colors[NamedColor::BrightForeground as usize] = to_rgb(theme.foreground);
        colors[NamedColor::DimForeground as usize] = dim(to_rgb(theme.foreground));
        Self {
            background,
            foreground: opaque(theme.foreground),
            selection: opaque(theme.selection),
            selected_text: opaque(theme.selection_foreground),
            colors,
        }
    }

    pub fn query_colors(&self) -> [Rgb; COUNT] {
        self.colors
    }

    pub fn resolve(&self, color: Color, overrides: &Colors) -> Hsla {
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

    pub fn foreground(&self, color: Color, flags: Flags, overrides: &Colors) -> Hsla {
        let resolved = self.resolve(color, overrides);
        if flags.contains(Flags::DIM) {
            from_rgb(dim(to_rgb(resolved)))
        } else {
            resolved
        }
    }

    pub fn background(&self, overrides: &Colors) -> Hsla {
        let mut background = self.resolve(Color::Named(NamedColor::Background), overrides);
        background.a = self.background.a;
        background
    }
}

fn opaque(mut color: Hsla) -> Hsla {
    color.a = 1.;
    color
}
fn to_rgb(color: Hsla) -> Rgb {
    let color = color.to_rgb();
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    Rgb {
        r: channel(color.r),
        g: channel(color.g),
        b: channel(color.b),
    }
}
fn from_rgb(color: Rgb) -> Hsla {
    rgb((u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)).into()
}
fn dim(color: Rgb) -> Rgb {
    let channel = |value: u8| (f32::from(value) * 0.66).round() as u8;
    Rgb {
        r: channel(color.r),
        g: channel(color.g),
        b: channel(color.b),
    }
}
fn mix(color: Hsla, target: Hsla, amount: f32) -> Hsla {
    let color = color.to_rgb();
    let target = target.to_rgb();
    gpui_kit::Rgba {
        r: color.r + (target.r - color.r) * amount,
        g: color.g + (target.g - color.g) * amount,
        b: color.b + (target.b - color.b) * amount,
        a: 1.,
    }
    .into()
}
