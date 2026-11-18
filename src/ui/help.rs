//! Shortcut reference anatomy shared by application help pages.
use super::container::container_traits;
use anti_webui::prelude::*;
use gds_uim::el;
macro_rules! part {
    ($name:ident, $base:expr) => {
        pub struct $name {
            base: Div,
        }
        impl Default for $name {
            fn default() -> Self {
                Self { base: $base }
            }
        }
        container_traits!($name);
    };
}
part!(
    HelpPage,
    el! { .div.{ flex; flex_col; p: 24.0_f32; overflow_y_scroll; flex_1; min_h: 0.0_f32 } }
);
part!(
    ShortcutSection,
    el! { .div.{ flex; flex_col; pb:24.0_f32 } }
);
part!(
    ShortcutHeading,
    el! { .div.{ font_weight: FontWeight::SEMIBOLD; pb:16.0_f32; dyn_style:s=>$(s.text_size(crate::app::Gds::current().appearance().typography().scaled(24.0))) } }
);
pub struct ShortcutRow {
    base: Div,
}
impl ShortcutRow {
    pub fn new(label: impl Into<String>, shortcut: impl Into<String>) -> Self {
        let label = label.into();
        let shortcut = shortcut.into();
        Self {
            base: el! { .div.{ flex; items_center; gap: 24.0_f32; py:8.0_f32; border_b:1.0_f32; dyn_style:s=>$(s.border_color(theme().border)) }.[
                label.text.{ flex_1 },
                $(ShortcutBadge::new(&shortcut)),
            ] },
        }
    }
}
container_traits!(ShortcutRow);

pub struct ShortcutBadge {
    base: Div,
}

impl ShortcutBadge {
    pub fn new(stroke: &str) -> Self {
        let keys = stroke.split(['-', '+', ' ']).filter(|key| !key.is_empty()).map(|key| {
            let label = match key {
                "ctrl" => "Ctrl".into(),
                "alt" => "Alt".into(),
                "shift" => "Shift".into(),
                "space" => "Space".into(),
                "escape" => "Esc".into(),
                "left" => "←".into(),
                "right" => "→".into(),
                "up" => "↑".into(),
                "down" => "↓".into(),
                "–" => return el! { .div.[ $(key.to_owned()).text ] },
                _ => {
                    let mut characters = key.chars();
                    let initial = characters.next().expect("nonempty key");
                    format!("{}{}", initial.to_uppercase(), characters.as_str())
                }
            };
            el! { .div.{
                flex; items_center; justify_center;
                h: 22.0_f32; min_w: 22.0_f32; px: 6.0_f32; rounded: 4.0_f32; border: 1.0_f32;
                whitespace_nowrap;
                dyn_style: s => $(s.bg(theme().surface).text_color(theme().text_muted).border_color(theme().border)
                    .text_size(crate::app::Gds::current().appearance().typography().scaled(12.0)));
            }.[label.text] }
        });
        Self {
            base: el! { .div.{ flex; items_center; flex_none; gap: 4.0_f32 }.[..keys] },
        }
    }
}
container_traits!(ShortcutBadge);
