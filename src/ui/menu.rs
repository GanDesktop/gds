//! Shared menu surfaces, command rows and in-window context menu placement.

use gds_uim::el;
use gpui_kit::{
    component::menu::{PopupMenu, PopupMenuItem},
    prelude::*,
    *,
};

use super::{Shortcut, ShortcutBadge};
use crate::appearance::tokens::SMALL;

pub const WIDTH: f32 = 320.;
pub const MAX_WIDTH: f32 = 500.;
pub const MAX_HEIGHT: f32 = 360.;
const MIN_WIDTH: f32 = 220.;
const ROW_MIN_WIDTH: f32 = 260.;
const SHORTCUT_GAP: f32 = 24.;
const EDGE_MARGIN: f32 = 8.;
const SURFACE_BORDER: f32 = 1.;
const DISABLED_OPACITY: f32 = 0.5;

/// Keep Kit's menu colors, icons, borders and item states across applications.
pub fn popup(menu: PopupMenu) -> PopupMenu {
    menu.min_w(px(MIN_WIDTH))
        .max_w(px(MAX_WIDTH))
        .max_h(px(MAX_HEIGHT))
        .scrollable(true)
}

/// Let context menus use the window's height before scrolling. Kit applies the
/// height limit to the items container, inside the popover's border.
pub fn popup_in(menu: PopupMenu, window: &Window) -> PopupMenu {
    let available = window.bounds().size.height - px(2. * (EDGE_MARGIN + SURFACE_BORDER));
    popup(menu).max_h(available.max(px(0.)))
}

/// A command label and its effective shortcut, with one shared spacing rule.
pub fn row(
    label: impl Into<SharedString>,
    shortcut: Option<Shortcut>,
) -> impl Fn(&mut Window, &mut App) -> AnyElement {
    row_with_state(label, shortcut, true)
}

/// Keep custom shortcut badges and the menu item's interaction state in sync.
pub fn item(
    label: impl Into<SharedString>,
    shortcut: Option<Shortcut>,
    enabled: bool,
) -> PopupMenuItem {
    PopupMenuItem::element(row_with_state(label.into(), shortcut, enabled)).disabled(!enabled)
}

fn row_with_state(
    label: impl Into<SharedString>,
    shortcut: Option<Shortcut>,
    enabled: bool,
) -> impl Fn(&mut Window, &mut App) -> AnyElement {
    let label = label.into();
    move |window, _| {
        let badge = shortcut
            .as_ref()
            .and_then(|shortcut| shortcut.binding(window))
            .map(|binding| {
                el! {
                    .div { # "shortcut"; test_support; flex_shrink_0; } [
                        $(ShortcutBadge::binding(binding)),
                    ]
                }
            });
        el! {
            .div {
                # $(label.clone());
                test_support;
                role: Role::Label;
                aria_label: $(label.clone());
                flex;
                items_center;
                w_full;
                min_w: ROW_MIN_WIDTH.px;
                gap: SHORTCUT_GAP.px;
                text_size: SMALL;
                opacity: $(if enabled { 1.0 } else { DISABLED_OPACITY });
            } [
                .div { # "label"; test_support; flex_1; min_w_0; truncate } [$(label.clone())],
                ..badge,
            ]
        }
        .into_any_element()
    }
}

/// Regular windows host context menus in their own overlay, including at edges.
pub fn overlay(menu: Entity<PopupMenu>, position: Point<Pixels>, window: &Window) -> AnyElement {
    el! {
        .anchored [
            .div {
                w: $(window.bounds().size.width);
                h: $(window.bounds().size.height);
                on_scroll: $(|_, _, cx| cx.stop_propagation());
            } [
                .anchored {
                    position: position;
                    snap_to_window_with_margin: EDGE_MARGIN.px;
                    anchor: Anchor::TopLeft;
                } [menu],
            ],
        ].deferred {
            with_priority: gpui_kit::base::POPUP_PRIORITY
        }
    }
    .into_any_element()
}
