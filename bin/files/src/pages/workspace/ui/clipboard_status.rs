//! Persistent file clipboard summary and its full path details.

use super::*;
use gds::appearance::tokens::TOOLTIP_DELAY;
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    prelude::*,
};

const ICON_SIZE: f32 = 12.;
const LABEL_MAX_WIDTH: f32 = 280.;
const DETAILS_WIDTH: f32 = 560.;
const DETAILS_MAX_HEIGHT: f32 = 320.;

pub(super) fn render(cx: &App) -> Option<AnyElement> {
    let snapshot = file_clipboard::current(cx)?;
    let (icon, title) = match snapshot.mode {
        file_clipboard::Mode::Copy => (IconName::Copy, i18n::text("clipboard-copied", cx)),
        file_clipboard::Mode::Cut => (IconName::Scissors, i18n::text("clipboard-cut", cx)),
    };
    let label: SharedString = if let [path] = snapshot.paths.as_slice() {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
            .into()
    } else {
        i18n::count("item-count", snapshot.paths.len(), cx)
    };
    let tooltip = |window: &mut Window, cx: &mut App| {
        Tooltip::element(|_, cx| match file_clipboard::current(cx) {
            Some(snapshot) => details(&snapshot, cx),
            None => div().into_any_element(),
        })
        .build(window, cx)
    };
    Some(
        el! {
            .div {
                # "file-clipboard-status"; role: Role::Status; aria_label: title;
                flex; items_center; gap_1; min_w_0; max_w: LABEL_MAX_WIDTH.px;
                tooltip_show_delay: TOOLTIP_DELAY; hoverable_tooltip: tooltip;
            } [
                icon.Icon::new { size: ICON_SIZE.px; flex_shrink_0; },
                .div { min_w_0; truncate; } [label],
            ]
        }
        .into_any_element(),
    )
}

fn details(snapshot: &file_clipboard::Snapshot, cx: &App) -> AnyElement {
    let title = i18n::text(
        match snapshot.mode {
            file_clipboard::Mode::Copy => "clipboard-copied",
            file_clipboard::Mode::Cut => "clipboard-cut",
        },
        cx,
    );
    let paths = snapshot.paths.iter().map(|path| {
        el! { .div { min_w_0; whitespace_normal; } [$(path.to_string_lossy().into_owned())] }
    });
    el! {
        .div { occlude; w: DETAILS_WIDTH.px; max_w_full; flex; flex_col; gap_2; py_1; } [
            .div { flex; items_center; gap_2; } [title, $(i18n::count("item-count", snapshot.paths.len(), cx))],
            .div {
                # "file-clipboard-paths"; min_w_0; max_h: DETAILS_MAX_HEIGHT.px; overflow_y_scroll;
                flex; flex_col; gap_1;
            } [..paths],
            @if (snapshot.can_cancel())
                .div { flex; items_center; gap_2; text_color: $(cx.theme().muted_foreground); } [
                    $(ui::ShortcutBadge::new(Command::Cancel)),
                    $(i18n::text("clipboard-cancel-cut", cx)),
                ],
            @end,
        ]
    }.into_any_element()
}
