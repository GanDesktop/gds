//! User notices and the current process's frontend diagnostic log.

use gds::app::AppExt as _;
pub(crate) use gds::service::logs as log;
use gds::ui::notifications::surface;

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

pub(crate) use surface::{content, wrap};

use gpui_kit::{
    Anchor, AnyWindowHandle, App, IntoElement, TestSupportExt,
    component::{
        ActiveTheme, WindowExt,
        notification::{Notification, NotificationDelivery, NotificationType},
    },
    prelude::*,
    px,
};
use log::Level;
use std::hash::{DefaultHasher, Hash, Hasher};

pub(crate) fn store(cx: &App) -> Option<log::Store> {
    cx.gds().ui().logs().store()
}

pub(crate) fn record(level: Level, title: &str, message: &str, cx: &mut App) {
    if let Some(store) = store(cx) {
        store.record(level, "gds_agents::ui", text(title, message));
    }
}

fn text(title: &str, message: &str) -> String {
    if title.is_empty() {
        message.to_owned()
    } else {
        format!("{title}\n{message}")
    }
}

struct Notice;

const MAX_NOTICES: usize = 4;
const COMPACT_HEIGHT: f32 = 160.;

fn compact_window(window: &gpui_kit::Window) -> bool {
    window.viewport_size().height < px(COMPACT_HEIGHT)
}

/// Record user feedback and show it in its originating window without creating
/// a native window or moving focus. A closed window leaves the log entry intact.
pub(crate) fn notify(
    parent: Option<AnyWindowHandle>,
    level: Level,
    title: &str,
    message: &str,
    cx: &mut App,
) {
    record(level, title, message, cx);
    let Some(handle) = parent.or_else(|| cx.active_window()) else {
        return;
    };
    let title = title.to_owned();
    let message = message.to_owned();
    let mut hash = DefaultHasher::new();
    (level.as_str(), &title, &message).hash(&mut hash);
    let key = format!("{:016x}", hash.finish());
    // Callers often still hold the originating window's update borrow.
    cx.defer(move |cx| {
        let _ = handle.update(cx, |_, window, cx| {
            let copy = text(&title, &message);
            let id = format!("notice-{key}");
            let compact_window = compact_window(window);
            let notice = Notification::new()
                .id1::<Notice>(key.clone())
                .with_type(match level {
                    Level::Warn => NotificationType::Warning,
                    Level::Error => NotificationType::Error,
                    _ => NotificationType::Info,
                })
                .placement(Anchor::TopRight)
                .delivery(NotificationDelivery::InApp)
                .autohide(true)
                .content(move |_, _, cx| {
                    let body = if compact_window {
                        copy.clone()
                    } else {
                        message.clone()
                    };
                    let body_id = id.clone();
                    let copy_id = format!("{id}-copy");
                    let hint = copy.clone();
                    gpui_kit::div()
                        .id(id.clone())
                        .test_support()
                        .role(gpui_kit::Role::Status)
                        .aria_label(copy.clone())
                        .flex()
                        .flex_col()
                        .gap_1()
                        .min_w_0()
                        .when(!compact_window, |view| {
                            view.child(
                                gpui_kit::div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(title.clone()),
                            )
                        })
                        .child(
                            gpui_kit::div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .min_w_0()
                                .child(
                                    gpui_kit::div()
                                        .id(format!("{body_id}-message"))
                                        .test_support()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(cx.theme().foreground)
                                        .hoverable_tooltip(move |window, cx| {
                                            crate::shared::ui::tooltip::view(
                                                hint.clone(),
                                                window,
                                                cx,
                                            )
                                        })
                                        .child(body.replace('\n', " ")),
                                )
                                .child(
                                    crate::shared::ui::copy_button(copy_id, copy.clone())
                                        .flex_shrink_0(),
                                ),
                        )
                        .into_any_element()
                });
            let limit = if compact_window { 1 } else { MAX_NOTICES };
            if let Some(overlay) = surface::notices(window, cx) {
                overlay.update(cx, |overlay, cx| {
                    if overlay.count(cx) < limit {
                        overlay.push(notice, window, cx);
                    }
                });
            } else if window.notifications(cx).len() < limit {
                window.push_notification(notice, cx);
            }
        });
    });
}
