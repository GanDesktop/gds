use zgui::prelude::*;
use zgui::reactive::Signal;
use zgui::runtime::try_use_clipboard;
use zgui::{css, view};

use super::{
    Level, MAX_CONTENT_HEIGHT, Notification, NotificationQueue, more_items_label, visible_details,
};
use crate::app::Gds;
use crate::ui::actions::Command;
use crate::ui::button::IconButtonProps;
use crate::ui::icon::{IconName, IconProps};

pub(super) const SHEET: &str = css!(
    r#"
    .gds-notification-card {
        display: flex;
        flex-direction: column;
        gap: 6px;
        padding: 10px 12px;
        border-radius: 8px;
        border: 1px solid var(--gds-border);
        background: var(--gds-panel);
        color: var(--gds-panel-foreground);
        box-shadow: 0 6px 20px rgba(0, 0, 0, 0.24);
        outline: 1px solid transparent;
        outline-offset: -1px;
        font-family: var(--gds-font-sans);
    }

    .gds-notification-card[data-focus-visible="true"] {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-notification-header {
        display: flex;
        align-items: flex-start;
        gap: 8px;
        min-width: 0;
    }

    .gds-notification-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        height: 20px;
        flex-shrink: 0;
        color: var(--gds-panel-foreground);
    }

    .gds-notification-card[data-level="info"] .gds-notification-icon {
        color: var(--gds-primary);
    }

    .gds-notification-card[data-level="success"] .gds-notification-icon {
        color: var(--gds-success);
    }

    .gds-notification-card[data-level="warning"] .gds-notification-icon {
        color: var(--gds-warning);
    }

    .gds-notification-card[data-level="error"] .gds-notification-icon {
        color: var(--gds-error);
    }

    .gds-notification-content {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        gap: 4px;
        overflow-y: auto;
    }

    .gds-notification-summary {
        width: 100%;
        min-width: 0;
        font-size: var(--gds-font-size-small, 14px);
        font-weight: 400;
        line-height: 1.4;
        color: var(--gds-panel-foreground);
        overflow-wrap: anywhere;
    }

    .gds-notification-card[data-has-details="true"] .gds-notification-summary {
        font-weight: 600;
    }

    .gds-notification-actions {
        display: flex;
        align-items: center;
        gap: 2px;
        flex-shrink: 0;
        margin-top: -4px;
        margin-right: -4px;
    }

    .gds-notification-detail-line {
        width: 100%;
        min-width: 0;
        font-size: var(--gds-font-size-xs, 12px);
        line-height: 1.35;
        color: var(--gds-muted-foreground);
        overflow-wrap: anywhere;
    }
    "#
);

fn level_icon(level: Level) -> IconName {
    match level {
        Level::Info => IconName::Info,
        Level::Success => IconName::CircleCheck,
        Level::Warning => IconName::TriangleAlert,
        Level::Error => IconName::CircleAlert,
    }
}

/// Renders one opaque `--gds-panel` notification card with AT-SPI `'notification'` (`Role::Alert`
/// outer card + `Role::Alert`/`Role::Status` inner content), `1px` inset keyboard contour, static
/// `"And N more items"` overflow (`DETAIL_LIMIT = 4`), and untruncated clipboard copy.
pub(super) fn render_notification(queue: NotificationQueue, entry: Notification) -> AnyView {
    let id = entry.id;
    let level = entry.report.level;
    let level_attr = level.as_str().to_owned();
    let live = level.a11y_live();
    let content_role = level.content_role();
    let icon = level_icon(level);
    let summary = entry.report.summary.clone();
    let copy_payload = entry.report.copy_text();
    let (shown_slice, remaining) = visible_details(&entry.report.details);
    let mut visible_rows: Vec<String> = shown_slice.to_vec();
    if remaining > 0 {
        visible_rows.push(more_items_label(remaining));
    }
    let has_details = !visible_rows.is_empty();

    let copy_label = Gds::try_current().map_or_else(
        || "Copy notification".to_owned(),
        |gds| gds.i18n().text("notification-copy"),
    );
    let close_label = Gds::try_current().map_or_else(
        || "Close current notification".to_owned(),
        |gds| gds.i18n().text("notification-close"),
    );

    let card_ref = NodeRef::new();
    queue.register_card_ref(id, card_ref);

    let max_content_px = format!("{MAX_CONTENT_HEIGHT}px");
    let is_current_target = Signal::derive_local(move || queue.target_id() == Some(id));

    AnyView::new(view! {
        column(
            node_ref = card_ref,
            class = "gds-notification-card",
            tabindex = Focus::Programmatic,
            a11y:role = Role::Alert,
            a11y:live = live,
            a11y:label = summary.clone(),
            attr:data-level = level_attr,
            attr:data-has-details = has_details.then(|| "true".to_owned()),
            attr:data-selected = move || {
                (queue.focused().get() && queue.selected_id().get() == Some(id))
                    .then(|| "true".to_owned())
            },
            attr:data-focus-visible = move || {
                (queue.focused().get()
                    && queue.focus_visible().get()
                    && queue.selected_id().get() == Some(id))
                .then(|| "true".to_owned())
            },
            on:pointer_enter = move |_| queue.set_hovered(id, true),
            on:pointer_leave = move |_| queue.set_hovered(id, false),
            on:pointer_down = move |_| queue.mark_pointer_press(id),
            on:focus_in = move |ev: &mut EventCx<'_, zgui::view::events::FocusIn>| {
                queue.mark_card_focused(id, ev.cause);
            },
        ) {
            row(class = "gds-notification-header") {
                box(class = "gds-notification-icon", a11y:hidden = true) {
                    Icon(name = icon)
                }
                column(
                    class = "gds-notification-content",
                    style:max-height = max_content_px,
                    a11y:role = content_role,
                    a11y:label = summary.clone(),
                ) {
                    text(class = "gds-notification-summary") {{summary}}
                    for entry in move || visible_rows.clone().into_iter().enumerate().collect::<Vec<_>>(), key = |entry: &(usize, String)| entry.0 {
                        text(class = "gds-notification-detail-line") {{entry.1}}
                    }
                }
                row(class = "gds-notification-actions") {
                    IconButton(
                        icon = IconName::Copy,
                        label = copy_label,
                        attr:tabindex = "-1",
                        on:click = move |ev: &mut EventCx<'_, zgui::view::events::Click>| {
                            ev.stop_propagation();
                            if let Some(clipboard) = try_use_clipboard() {
                                clipboard.set_text(ClipboardKind::Standard, copy_payload.clone());
                            }
                        }
                    )
                    IconButton(
                        icon = IconName::X,
                        label = close_label,
                        attr:tabindex = "-1",
                        command = Command::CloseNotification,
                        shortcut_enabled = is_current_target,
                        on:click = move |ev: &mut EventCx<'_, zgui::view::events::Click>| {
                            ev.stop_propagation();
                            queue.dismiss(id);
                        }
                    )
                }
            }
        }
    })
}
