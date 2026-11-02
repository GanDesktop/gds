//! Suite-wide toast notifications for completed actions and failures.
//!
//! Kit's built-in notification layer stacks its toasts sonner-style: only the
//! newest card is readable and the older ones stay tucked behind it until the
//! pointer expands the stack. GDS reports failures that must remain readable
//! next to later successes, so this module owns a Kit [`NotificationList`]
//! queue and renders its mounted cards itself as a flat, always-expanded
//! column. Queue, lifetime, close and id-replacement behaviour still come from
//! Kit; only the presentation is ours.

use crate::appearance::tokens;
mod interaction;
pub mod surface;
pub use interaction::{close_all, close_current, focus, is_focused};

use std::collections::HashSet;

use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::base::ScrollableMask;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::{Notification, NotificationList};
use gpui_kit::component::{ActiveTheme, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext, Axis, ClickEvent, ClipboardItem, Context, Div,
    ElementId, Entity, EntityId, FocusHandle, FontWeight, Global, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TestSupportExt as _, Window, div,
    px,
};

use crate::{i18n, i18n::t, ui};

pub use gpui_kit::component::notification::NotificationType as Level;

/// Horizontal inset of the toast column from the window's right edge.
const TOAST_INSET: f32 = tokens::NOTIFICATION_INSET;
/// Gap between the toolbar and the first toast.
const TOAST_TOP_GAP: f32 = tokens::NOTIFICATION_TOP_GAP;
/// Vertical gap between two expanded toasts.
const TOAST_GAP: f32 = 8.;
/// Failed items listed inside one toast; the rest collapse into a count.
const DETAIL_LIMIT: usize = 4;
const DETAIL_GAP: f32 = 4.;

/// Tallest the scrollable text area of one toast may grow.
///
/// A summary quotes whole external errors, so its length cannot be assumed:
/// the area scrolls instead of stretching the card past the window bottom.
pub const MAX_CONTENT_HEIGHT: f32 = 240.;
/// Short windows shrink the text area down to this; a window too short for even
/// that has no room for a toast either way.
const MIN_CONTENT_HEIGHT: f32 = 72.;
/// Vertical space Kit's card spends around the text area: its `py_3p5` padding
/// and one-pixel border on each side.
const CARD_CHROME_HEIGHT: f32 = 30.;
/// The id every toast text area shares, so its scroll position and hit area
/// survive re-renders. Each notification's own wrapper scopes it.
const CONTENT_ID: &str = "gds-notification-content";
const COMPACT_VIEWPORT_HEIGHT: f32 = 160.;
/// Wheel mask over one toast's text area.
///
/// Kit's card occludes the pointer, so an ancestor scroll area never sees the
/// wheel while the pointer is over a card. A mask painted above the text keeps
/// the card's hover and close button intact but routes the wheel to the text
/// area, then to the column once this toast reaches its end.
const CONTENT_MASK_ID: &str = "gds-notification-content-wheel";
/// Wheel mask over the whole expanded column.
const COLUMN_MASK_ID: &str = "gds-notification-column-wheel";
/// Window space above and below one toast's text area.
const CONTENT_RESERVED_HEIGHT: f32 =
    tokens::TOOLBAR_HEIGHT + TOAST_TOP_GAP + TOAST_INSET + CARD_CHROME_HEIGHT;

#[derive(Default)]
struct Host {
    windows: std::collections::HashMap<gpui_kit::WindowId, WindowHost>,
}
struct WindowHost {
    window: AnyWindowHandle,
    list: Entity<NotificationList>,
    overlay: Entity<Overlay>,
}
impl Global for Host {}

/// Register an independent queue for this window. Repeated registration keeps its notices.
pub fn bind_window(window: &mut Window, cx: &mut App) {
    if !cx.has_global::<Host>() {
        cx.set_global(Host::default());
        cx.on_window_closed(|cx, id| {
            cx.global_mut::<Host>().windows.remove(&id);
        })
        .detach();
    }
    let id = window.window_handle().window_id();
    if cx
        .try_global::<Host>()
        .is_some_and(|host| host.windows.contains_key(&id))
    {
        return;
    }
    let list = cx.new(|cx| NotificationList::new(window, cx));
    let overlay = cx.new(|cx| Overlay::new(list.clone(), window, cx));
    let live = cx.windows();
    let host = cx.default_global::<Host>();
    host.windows.retain(|_, entry| live.contains(&entry.window));
    host.windows.insert(
        id,
        WindowHost {
            window: window.window_handle(),
            list,
            overlay,
        },
    );
}

pub fn overlay_for(window: &Window, cx: &App) -> Option<Entity<Overlay>> {
    cx.try_global::<Host>()?
        .windows
        .get(&window.window_handle().window_id())
        .map(|host| host.overlay.clone())
}

/// The active window's queue, for application-level callers without a window borrow.
pub fn overlay(cx: &App) -> Option<Entity<Overlay>> {
    let window = cx
        .active_window()
        .or_else(|| cx.windows().first().copied())?;
    cx.try_global::<Host>()?
        .windows
        .get(&window.window_id())
        .map(|host| host.overlay.clone())
}

fn host(window: AnyWindowHandle, cx: &App) -> Option<Entity<NotificationList>> {
    if !cx.windows().contains(&window) {
        return None;
    }
    cx.try_global::<Host>()?
        .windows
        .get(&window.window_id())
        .map(|host| host.list.clone())
}

/// Mount a notification in the host window.
///
/// Callers usually run inside that window's own update, so the push is
/// deferred: re-entering the window borrow immediately would panic. A closed
/// window is reported instead of silently dropping the notification.
fn push(note: Notification, cx: &mut App) {
    let target = cx.active_window().or_else(|| cx.windows().first().copied());
    push_to(note, target, cx);
}

fn push_to(note: Notification, target: Option<AnyWindowHandle>, cx: &mut App) {
    // Kit's card paints the popover surface but leaves text to inheritance;
    // set the matching semantic foreground so the toast stays readable on
    // every theme.
    let note = note.text_color(cx.theme().popover_foreground);
    cx.defer(move |cx| {
        let Some(window) = target.filter(|target| cx.windows().contains(target)) else {
            return;
        };
        if let Some(list) = host(window, cx) {
            if let Err(error) = window.update(cx, |_, window, cx| {
                list.update(cx, |list, cx| list.push(note, window, cx));
            }) {
                eprintln!("Cannot show a notification: {error}");
            }
            return;
        }
        // An application that never bound its window keeps Kit's default layer.
        if let Err(error) = window.update(cx, |_, window, cx| {
            window.push_notification(note, cx);
        }) {
            eprintln!("Cannot show a notification: {error}");
        }
    });
}

/// Height the text area may take in a window of `viewport` height.
///
/// The card starts below the toolbar and must leave the bottom inset free, so
/// a short window gets a smaller area rather than a toast running off-screen.
fn content_height(viewport: Pixels) -> Pixels {
    (viewport - px(CONTENT_RESERVED_HEIGHT))
        .max(px(MIN_CONTENT_HEIGHT))
        .min(px(MAX_CONTENT_HEIGHT))
}

/// Room the whole expanded column may take inside a window of `viewport`
/// height, below the toolbar and above the bottom inset.
///
/// Several tall reports together exceed any window; the column scrolls inside
/// this height instead of growing past the bottom edge.
fn column_height(viewport: Pixels, top: Pixels, bottom: Pixels) -> Pixels {
    (viewport - top - bottom).max(px(0.))
}

pub fn info(message: impl Into<SharedString>, cx: &mut App) {
    Report::new(Level::Info, message).show(cx);
}

pub fn success(message: impl Into<SharedString>, cx: &mut App) {
    Report::new(Level::Success, message).show(cx);
}

pub fn warning(message: impl Into<SharedString>, cx: &mut App) {
    Report::new(Level::Warning, message).show(cx);
}

/// Show a failure.
///
/// Errors stay mounted until closed: a failed file operation usually needs to
/// be read, and Kit only offers a five-second automatic dismissal.
pub fn error(message: impl Into<SharedString>, cx: &mut App) {
    Report::new(Level::Error, message).show(cx);
}

/// A notification with a summary and the failed items behind it.
pub struct Report {
    level: Level,
    summary: SharedString,
    details: Vec<SharedString>,
    identity: Option<fn(Notification) -> Notification>,
}

impl Report {
    pub fn new(level: Level, summary: impl Into<SharedString>) -> Self {
        Self {
            level,
            summary: summary.into(),
            details: Vec::new(),
            identity: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.details.push(detail.into());
        self
    }

    pub fn details(mut self, details: impl IntoIterator<Item = String>) -> Self {
        self.details
            .extend(details.into_iter().map(SharedString::from));
        self
    }

    /// Reuse slot `T` so a repeated report replaces its predecessor instead of
    /// stacking near-identical toasts.
    pub fn replacing<T: 'static>(mut self) -> Self {
        self.identity = Some(Notification::id::<T>);
        self
    }

    pub fn show(self, cx: &mut App) {
        let target = cx.active_window().or_else(|| cx.windows().first().copied());
        self.show_to(target, cx);
    }

    /// Deliver a background result to its originating window. A closed origin
    /// discards the toast without moving it to another window.
    pub fn show_in(self, window: AnyWindowHandle, cx: &mut App) {
        self.show_to(Some(window), cx);
    }

    fn show_to(self, target: Option<AnyWindowHandle>, cx: &mut App) {
        match self.level {
            Level::Error => {
                tracing::error!(target: "gds::ui::notifications", details = ?self.details, "{}", self.summary)
            }
            Level::Warning => {
                tracing::warn!(target: "gds::ui::notifications", details = ?self.details, "{}", self.summary)
            }
            _ => {
                tracing::info!(target: "gds::ui::notifications", details = ?self.details, "{}", self.summary)
            }
        }
        let mut note = self
            .notification()
            .autohide(!matches!(self.level, Level::Error));
        if let Some(identity) = self.identity {
            note = identity(note);
        }
        push_to(note, target, cx);
    }

    /// Kit's notification for this report, before lifetime and slot policy.
    fn notification(&self) -> Notification {
        Notification::new()
            .with_type(self.level)
            .content(self.body())
            .action(self.copy_action())
    }

    fn copy_action(
        &self,
    ) -> impl Fn(&mut Notification, &mut Window, &mut Context<Notification>) -> Button + 'static
    {
        let text = std::iter::once(&self.summary)
            .chain(&self.details)
            .map(|line| line.as_ref())
            .collect::<Vec<_>>()
            .join("\n");
        move |_, _, cx| {
            let text = text.clone();
            let on_copy = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                cx.stop_propagation();
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            };
            el! {
                "copy-notification".Button::new {
                    ghost; icon: IconName::Copy;
                    tooltip: t!(@cx, "notification-copy");
                    accessibility_label: t!(@cx, "notification-copy");
                    on_click: on_copy;
                }
            }
        }
    }

    /// The summary and details in one height-bounded, scrollable area.
    ///
    /// Both may be arbitrarily long, so the area -- not the entry count -- is
    /// what keeps the card inside the window; everything stays reachable by
    /// scrolling rather than being cut off.
    fn body(
        &self,
    ) -> impl Fn(&mut Notification, &mut Window, &mut Context<Notification>) -> AnyElement + 'static
    {
        let summary = self.summary.clone();
        let details = self.details.clone();
        // Kept with the content builder so the scroll position outlives one
        // notification render.
        let scroll = ScrollHandle::new();
        move |_, window, cx| {
            let muted = cx.theme().muted_foreground;
            let height = content_height(window.viewport_size().height);
            let row = |text: SharedString| -> Div {
                el! { .div { w_full; min_w_0; text_xs; text_color: muted } [text] }
            };
            let (shown, remaining) = visible_details(&details);
            let mut rows: Vec<Div> = shown.iter().cloned().map(&row).collect();
            if remaining > 0 {
                rows.push(row(i18n::count("operation-more-items", remaining, cx)));
            }
            // A report leads with its summary; a plain message is the whole toast.
            let summary_weight = if rows.is_empty() {
                FontWeight::NORMAL
            } else {
                FontWeight::SEMIBOLD
            };
            let text = el! {
                .div {
                    # CONTENT_ID;
                    flex; flex_col; gap: DETAIL_GAP.px;
                    w_full; min_w_0;
                    max_h: height;
                    overflow_y_scroll;
                    track_scroll: $(&scroll);
                } [
                    .div {
                        w_full; min_w_0; text_sm;
                        font_weight: summary_weight;
                    } [$(summary.clone())],
                    ..rows,
                ]
            };
            el! {
                .div { relative; w_full; min_w_0; max_h: height } [
                    text,
                    $(ScrollableMask::new(Axis::Vertical, &scroll).id(CONTENT_MASK_ID)),
                ]
            }
            .into_any_element()
        }
    }
}

/// The visible subset of a report's details and the count left out.
fn visible_details(details: &[SharedString]) -> (&[SharedString], usize) {
    let shown = details.len().min(DETAIL_LIMIT);
    (&details[..shown], details.len() - shown)
}

/// The single reusable status toast of a background operation.
pub struct Operation;

impl Operation {
    /// Announce work that may outlive the current frame.
    pub fn start(message: impl Into<SharedString>, cx: &mut App) {
        push(
            Report::new(Level::Info, message)
                .notification()
                .id::<Operation>()
                .autohide(false),
            cx,
        );
    }

    /// Replace the progress toast with the operation's outcome.
    pub fn finish(report: Report, cx: &mut App) {
        report.replacing::<Operation>().show(cx);
    }
}

/// Flat, always-expanded presentation of the suite's notification queue.
///
/// Kit's [`NotificationList`] owns the mounted entities, the enter/exit clock,
/// auto-hide, dismiss requests and id replacement. Rendering its
/// [`NotificationList::notifications`] directly, instead of the list itself,
/// skips the sonner `ToastStack`: cards keep their natural height in a column
/// rather than overlapping behind the newest one, and the stack never reports
/// hover/focus expansion, so auto-hide keeps counting down.
pub struct Overlay {
    list: Entity<NotificationList>,
    scroll: ScrollHandle,
    focus: FocusHandle,
    return_focus: Option<FocusHandle>,
    selected: Option<EntityId>,
    selected_index: usize,
    closing: HashSet<EntityId>,
    _list_changes: Subscription,
}

impl Overlay {
    /// Create an independent notification queue for a window.
    /// Multi-window applications retain and mount this entity in that window.
    pub fn for_window(window: &mut Window, cx: &mut App) -> Entity<Self> {
        bind_window(window, cx);
        overlay_for(window, cx).expect("registered window owns a notification overlay")
    }

    /// Number of mounted notices, including ones finishing their close animation.
    pub fn count(&self, cx: &App) -> usize {
        self.list.read(cx).notifications().len()
    }

    /// Mount or replace a notice in this window's queue.
    pub fn push(&mut self, note: Notification, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| list.push(note, window, cx));
    }

    fn new(list: Entity<NotificationList>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _list_changes = cx.observe_in(&list, window, |this, _, window, cx| {
            this.sync_selection(window, cx);
            cx.notify();
        });
        Self {
            list,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            return_focus: None,
            selected: None,
            selected_index: 0,
            closing: HashSet::new(),
            _list_changes,
        }
    }
}

impl Render for Overlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notes = self.list.read(cx).notifications();
        if notes.is_empty() {
            // Nothing to show: do not leave an empty overlay over the files.
            return div().into_any_element();
        }
        let settings = &cx.theme().notification;
        let (top, right, bottom) = (
            settings.margins.top,
            settings.margins.right,
            settings.margins.bottom,
        );
        // A compact popup has no toolbar and still needs room for a whole card.
        let top = if window.viewport_size().height < px(COMPACT_VIEWPORT_HEIGHT) {
            px(4.)
        } else {
            top
        };
        let right = right.min(window.viewport_size().width / 2.);
        let width = settings
            .width
            .min(window.viewport_size().width - right * 2.);
        let height = column_height(window.viewport_size().height, top, bottom);
        let active = self.focus.contains_focused(window, cx);
        let focus_visible = ui::region::is_focus_visible(&self.focus, window);
        let target = if active {
            self.selected
        } else {
            self.available(cx).last().map(|note| note.entity_id())
        };
        let cards: Vec<_> = notes
            .into_iter()
            .map(|note| {
                let id = ElementId::from(("gds-notification", note.entity_id()));
                let current = target == Some(note.entity_id());
                let close_hint = current.then(|| {
                    ui::keytips::anchor(
                        window,
                        cx,
                        ("notification-close-hint", note.entity_id()).into(),
                        crate::ui::actions::Command::CloseNotification.into(),
                    )
                });
                div()
                    .id(id)
                    .test_support()
                    .relative()
                    .w_full()
                    .flex_shrink_0()
                    .child(note)
                    .children(close_hint)
                    .when(focus_visible && current, |card| {
                        card.child(ui::region::focus_ring(cx))
                    })
            })
            .collect();
        let focus_hint = ui::keytips::anchor(
            window,
            cx,
            "notifications-focus-hint".into(),
            crate::ui::actions::Command::FocusNotifications.into(),
        );
        let close_all = ui::keytips::anchor(
            window,
            cx,
            "notifications-close-all-hint".into(),
            crate::ui::actions::Command::CloseAllNotifications.into(),
        );
        el! {
            .div {
                # "gds-notification-overlay";
                track_focus: $&self.focus;
                key_context: "Notifications";
                on_key_down: (cx, Self::on_key_down).Context::listener;
                on_action: (cx, Self::on_cancel).Context::listener;
                absolute; top: top; right: right;
                w: width; max_h: height;
                flex; flex_col;
            } [
                .div {
                    # "gds-notification-column";
                    w_full; max_h: height;
                    flex; flex_col; gap: TOAST_GAP.px;
                    overflow_y_scroll;
                    track_scroll: $(&self.scroll);
                } [..cards],
                (Axis::Vertical, $&self.scroll).ScrollableMask::new {
                    # COLUMN_MASK_ID
                },
                focus_hint,
                close_all,
            ]
        }
        .into_any_element()
    }
}

#[cfg(test)]
mod tests;
