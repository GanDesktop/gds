//! Read-only help content. Shortcut badges always resolve the live keymap.
use super::{Shortcut, ShortcutBadge, container::container_traits};
use crate::app::AppExt as _;
use gds_uim::el;
use gpui_kit::{
    IntoElement,
    component::{ActiveTheme, StyledExt},
    prelude::*,
    *,
};

gpui_kit::actions!(help, [OpenHelp]);

#[derive(IntoElement)]
pub struct HelpPage {
    base: Stateful<Div>,
}
impl Default for HelpPage {
    fn default() -> Self {
        Self {
            base: el! { .div { # "help-page"; size_full; min_h_0; overflow_y_scroll; p_6; } },
        }
    }
}
container_traits!(HelpPage);
impl RenderOnce for HelpPage {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        self.base
            .bg(cx.gds().appearance().theme().content_background())
            .test_support()
    }
}

#[derive(IntoElement)]
pub struct ShortcutSection {
    base: Div,
}
impl Default for ShortcutSection {
    fn default() -> Self {
        Self {
            base: el! { .div { pb_6; } },
        }
    }
}
container_traits!(ShortcutSection);
impl RenderOnce for ShortcutSection {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}

#[derive(IntoElement)]
pub struct ShortcutHeading {
    base: Div,
}
impl Default for ShortcutHeading {
    fn default() -> Self {
        Self {
            base: el! { .div { text_2xl; font_semibold; pb_4; } },
        }
    }
}
container_traits!(ShortcutHeading);
impl RenderOnce for ShortcutHeading {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}

#[derive(IntoElement)]
pub struct ShortcutRow {
    label: SharedString,
    shortcut: Shortcut,
    range_end: Option<Shortcut>,
}
impl ShortcutRow {
    pub fn new(label: impl Into<SharedString>, shortcut: impl Into<Shortcut>) -> Self {
        Self {
            label: label.into(),
            shortcut: shortcut.into(),
            range_end: None,
        }
    }
    pub fn range(mut self, last: impl Into<Shortcut>) -> Self {
        self.range_end = Some(last.into());
        self
    }
}
impl RenderOnce for ShortcutRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        el! {
            .div { flex; items_center; justify_between; gap_6; py_2; border_b_1; border_color: $(cx.theme().border); } [
                .div { flex_1; min_w_0; } [$self.label],
                .div { flex; items_center; gap_2; } [
                    $(self.shortcut).ShortcutBadge::bound,
                    @if (let Some(last) = self.range_end)
                        "–",
                        last.ShortcutBadge::bound,
                    @end,
                ],
            ]
        }
    }
}
