//! Suite-wide expanded toast notifications emitting AT-SPI `'notification'` (`Role::Alert`).

mod interaction;
pub mod surface;
#[cfg(test)]
mod tests;

use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::time::Duration;

pub use interaction::{close_all, close_current, focus, is_focused};
use zgui::prelude::*;
use zgui::reactive::{
    LocalStorage, RenderEffect, RwSignal, Signal, on_cleanup_local, provide_local_context,
    use_local_context,
};
use zgui::view::time::Timers;
use zgui::view::{
    HostHandle, ListenerGuard, OverlayLayer, PortalProps, ShowProps, TimeoutHandle, current_host,
    events,
};
use zgui::vocab::{FocusCause, Key, Live, Modifiers, NamedKey, Role};
use zgui::{component, css, view};

use crate::app::Gds;
use crate::appearance::tokens::{NOTIFICATION_TOP_GAP, TOOLBAR_HEIGHT};
use crate::ui::actions::{ActionDispatcher, Command};
use crate::ui::keytips::{
    HintPlacement, has_active_modal_scope, register_occlusion_surface, stage_shortcut,
};

pub const TOP_OFFSET: f32 = TOOLBAR_HEIGHT + NOTIFICATION_TOP_GAP;
pub const MAX_CONTENT_HEIGHT: f32 = 240.0;
pub const DETAIL_LIMIT: usize = 4;
pub const AUTO_DISMISS_DELAY: Duration = Duration::from_secs(5);

const HOST_SHEET: &str = css!(
    r#"
    .gds-notification-root {
        display: contents;
    }

    .gds-notification-column {
        position: fixed;
        top: calc(var(--gds-toolbar-height, 48px) + var(--gds-notification-top-gap, 8px));
        right: var(--gds-notification-inset, 12px);
        display: flex;
        flex-direction: column;
        gap: 8px;
        width: 360px;
        max-width: calc(100vw - 24px);
        max-height: calc(100vh - 72px);
        overflow-x: hidden;
        overflow-y: auto;
        pointer-events: auto;
    }
    "#
);

/// Severity level for a user-visible notification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl Level {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    #[must_use]
    pub const fn a11y_live(self) -> Live {
        match self {
            Self::Error | Self::Warning => Live::Assertive,
            Self::Info | Self::Success => Live::Polite,
        }
    }

    #[must_use]
    pub const fn content_role(self) -> Role {
        match self {
            Self::Error | Self::Warning => Role::Alert,
            Self::Info | Self::Success => Role::Status,
        }
    }

    /// Info, Success, and Warning autohide after [`AUTO_DISMISS_DELAY`]; only `Error` stays
    /// mounted until explicitly dismissed (`main:notifications.rs:1360`).
    #[must_use]
    pub const fn default_autohide(self) -> bool {
        !matches!(self, Self::Error)
    }
}

/// Structured notification payload with summary and optional detail lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub level: Level,
    pub summary: String,
    pub details: Vec<String>,
    slot: Option<TypeId>,
    autohide_override: Option<bool>,
}

impl Report {
    /// Creates a report with `level` and primary `summary` text.
    #[must_use]
    pub fn new(level: Level, summary: impl Into<String>) -> Self {
        Self {
            level,
            summary: summary.into(),
            details: Vec::new(),
            slot: None,
            autohide_override: None,
        }
    }

    /// Appends one detail line.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.details.push(detail.into());
        self
    }

    /// Appends multiple detail lines.
    #[must_use]
    pub fn details(mut self, details: impl IntoIterator<Item = String>) -> Self {
        self.details.extend(details);
        self
    }

    /// Marks this report as replacing any live notification tagged with slot type `T`.
    #[must_use]
    pub fn replacing<T: 'static>(mut self) -> Self {
        self.slot = Some(TypeId::of::<T>());
        self
    }

    /// Overrides whether the notification automatically dismisses after [`AUTO_DISMISS_DELAY`].
    #[must_use]
    pub fn autohide(mut self, autohide: bool) -> Self {
        self.autohide_override = Some(autohide);
        self
    }

    /// Publishes this report to the active window's notification stack.
    pub fn show(self) {
        report(self);
    }

    /// Formats summary and all untruncated detail lines as plain text for clipboard copying.
    #[must_use]
    pub fn copy_text(&self) -> String {
        if self.details.is_empty() {
            return self.summary.clone();
        }
        let mut out = self.summary.clone();
        for detail in &self.details {
            out.push('\n');
            out.push_str(detail);
        }
        out
    }
}

/// Reusable status toast slot and batch builder for background operations (`main:notifications.rs`).
#[derive(Clone, Debug, Default)]
pub struct Operation {
    summary: String,
    details: Vec<String>,
    level: Level,
    slot: Option<TypeId>,
}

impl Operation {
    /// Announces long-running background work in the shared [`Operation`] replacement slot without
    /// auto-dismissing while in flight.
    pub fn start(message: impl Into<String>) {
        Report::new(Level::Info, message)
            .replacing::<Self>()
            .autohide(false)
            .show();
    }

    /// Replaces the in-flight [`Operation`] progress toast with its final outcome `report`.
    pub fn finish(report: Report) {
        report.replacing::<Self>().show();
    }

    /// Starts a batch operation report summary.
    #[must_use]
    pub fn new(level: Level, summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            details: Vec::new(),
            level,
            slot: None,
        }
    }

    /// Adds an item line to the operation report.
    #[must_use]
    pub fn item(mut self, line: impl Into<String>) -> Self {
        self.details.push(line.into());
        self
    }

    /// Tags this operation report with a replacement slot type `T`.
    #[must_use]
    pub fn replacing<T: 'static>(mut self) -> Self {
        self.slot = Some(TypeId::of::<T>());
        self
    }

    /// Converts this operation summary into a [`Report`].
    #[must_use]
    pub fn into_report(self) -> Report {
        let mut report = Report::new(self.level, self.summary).details(self.details);
        report.slot = self.slot;
        report
    }
}

/// Returns the visible slice (`<= DETAIL_LIMIT`) and remaining overflow count of `details`.
#[must_use]
pub(crate) fn visible_details(details: &[String]) -> (&[String], usize) {
    let shown = details.len().min(DETAIL_LIMIT);
    (&details[..shown], details.len() - shown)
}

/// Formats the static localized `"And N more items"` overflow line for truncated detail lists.
#[must_use]
pub(crate) fn more_items_label(remaining: usize) -> String {
    Gds::try_current().map_or_else(
        || format!("And {remaining} more items"),
        |gds| {
            gds.i18n()
                .count_untracked("operation-more-items", remaining)
        },
    )
}

/// Queued notification entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub id: u64,
    pub report: Report,
    pub autohide: bool,
}

#[derive(Default)]
struct QueueRuntime {
    timers: HashMap<u64, TimeoutHandle>,
    slot_ids: HashMap<TypeId, u64>,
    card_refs: HashMap<u64, NodeRef>,
    host_ref: Option<NodeRef>,
    host_handle: Option<HostHandle>,
    timers_api: Option<Timers>,
    previous_focus: Option<NodeId>,
}

#[derive(Clone, Copy)]
pub(crate) struct NotificationQueue {
    items: RwSignal<Vec<Notification>, LocalStorage>,
    next_id: RwSignal<u64, LocalStorage>,
    focused: RwSignal<bool, LocalStorage>,
    focus_visible: RwSignal<bool, LocalStorage>,
    selected_id: RwSignal<Option<u64>, LocalStorage>,
    hovered_ids: RwSignal<BTreeSet<u64>, LocalStorage>,
    runtime: RwSignal<Option<Rc<RefCell<QueueRuntime>>>, LocalStorage>,
}

thread_local! {
    static MOUNTED_HOSTS: RefCell<Vec<(u64, NotificationQueue)>> = const { RefCell::new(Vec::new()) };
    static NEXT_HOST_ID: Cell<u64> = const { Cell::new(0) };
    static FALLBACK_QUEUE: RefCell<Option<NotificationQueue>> = const { RefCell::new(None) };
}

impl NotificationQueue {
    fn new() -> Self {
        let rt = Rc::new(RefCell::new(QueueRuntime::default()));
        Self {
            items: RwSignal::new_local(Vec::new()),
            next_id: RwSignal::new_local(1),
            focused: RwSignal::new_local(false),
            focus_visible: RwSignal::new_local(false),
            selected_id: RwSignal::new_local(None),
            hovered_ids: RwSignal::new_local(BTreeSet::new()),
            runtime: RwSignal::new_local(Some(rt)),
        }
    }

    fn is_alive(self) -> bool {
        self.next_id.try_get_untracked().is_some()
    }

    fn runtime_rc(self) -> Option<Rc<RefCell<QueueRuntime>>> {
        self.runtime.try_get_untracked().flatten()
    }

    pub(crate) fn active() -> Self {
        if zgui::reactive::is_ui_thread()
            && let Some(queue) = use_local_context::<Self>().filter(|q| q.is_alive())
        {
            return queue;
        }
        if let Some(mounted) = MOUNTED_HOSTS.with(|hosts| {
            let mut list = hosts.borrow_mut();
            list.retain(|&(_, q)| q.is_alive());
            list.last().map(|&(_, q)| q)
        }) {
            return mounted;
        }
        FALLBACK_QUEUE.with(|slot| {
            if let Some(queue) = *slot.borrow()
                && queue.is_alive()
            {
                return queue;
            }
            let queue = Self::new();
            *slot.borrow_mut() = Some(queue);
            queue
        })
    }

    fn register_host_ref(self, node_ref: NodeRef) {
        if let Some(rt) = self.runtime_rc() {
            let mut rt_mut = rt.borrow_mut();
            rt_mut.host_ref = Some(node_ref);
            rt_mut.host_handle = current_host();
            rt_mut.timers_api = Timers::current();
        }
    }

    fn host_handle(self) -> Option<HostHandle> {
        self.runtime_rc()
            .and_then(|rt| rt.borrow().host_handle.clone())
            .or_else(|| zgui::reactive::is_ui_thread().then(current_host).flatten())
    }

    fn timers_api(self) -> Option<Timers> {
        self.runtime_rc()
            .and_then(|rt| rt.borrow().timers_api.clone())
            .or_else(|| {
                zgui::reactive::is_ui_thread()
                    .then(Timers::current)
                    .flatten()
            })
    }

    pub(crate) fn register_card_ref(self, id: u64, node_ref: NodeRef) {
        if let Some(rt) = self.runtime_rc() {
            rt.borrow_mut().card_refs.insert(id, node_ref);
        }
    }

    pub(crate) fn push(self, report: Report) {
        match report.level {
            Level::Error => {
                tracing::error!(
                    target: "gds::ui::notifications",
                    details = ?report.details,
                    "{}",
                    report.summary
                );
            }
            Level::Warning => {
                tracing::warn!(
                    target: "gds::ui::notifications",
                    details = ?report.details,
                    "{}",
                    report.summary
                );
            }
            Level::Info | Level::Success => {
                tracing::info!(
                    target: "gds::ui::notifications",
                    details = ?report.details,
                    "{}",
                    report.summary
                );
            }
        }

        let slot = report.slot;
        let autohide = report
            .autohide_override
            .unwrap_or_else(|| report.level.default_autohide());

        let replaced_id = slot.and_then(|slot_key| {
            self.runtime_rc()
                .and_then(|rt| rt.borrow().slot_ids.get(&slot_key).copied())
        });

        let id = if let Some(existing_id) = replaced_id
            && self
                .items
                .get_untracked()
                .iter()
                .any(|item| item.id == existing_id)
        {
            self.cancel_timer(existing_id);
            self.items.update(|items| {
                if let Some(entry) = items.iter_mut().find(|item| item.id == existing_id) {
                    entry.report = report;
                    entry.autohide = autohide;
                }
            });
            existing_id
        } else {
            let id = self.next_id.get_untracked();
            self.next_id.set(id.saturating_add(1));
            self.items.update(|items| {
                items.insert(
                    0,
                    Notification {
                        id,
                        report,
                        autohide,
                    },
                );
            });
            if let Some(slot_key) = slot
                && let Some(rt) = self.runtime_rc()
            {
                rt.borrow_mut().slot_ids.insert(slot_key, id);
            }
            id
        };

        if self.focused.get_untracked() && self.selected_id.get_untracked().is_none() {
            self.selected_id.set(Some(id));
        }
        self.schedule_autohide_if_eligible(id);
    }

    pub(crate) fn dismiss(self, id: u64) {
        self.cancel_timer(id);

        let mut next_focus_target: Option<(u64, NodeRef)> = None;
        let mut restore_node: Option<NodeId> = None;

        let prev_index = self
            .items
            .get_untracked()
            .iter()
            .position(|item| item.id == id)
            .unwrap_or(0);

        self.items.update(|items| {
            items.retain(|item| item.id != id);
        });
        self.hovered_ids.update(|set| {
            set.remove(&id);
        });

        let remaining = self.items.get_untracked();
        if self.selected_id.get_untracked() == Some(id) {
            let next_id = if remaining.is_empty() {
                None
            } else {
                Some(remaining[prev_index.min(remaining.len() - 1)].id)
            };
            self.selected_id.set(next_id);
        }

        if let Some(rt) = self.runtime_rc() {
            let mut rt_mut = rt.borrow_mut();
            rt_mut.card_refs.remove(&id);
            rt_mut.slot_ids.retain(|_, existing| *existing != id);
            if remaining.is_empty() {
                restore_node = rt_mut.previous_focus.take();
            } else if self.focused.get_untracked()
                && let Some(target_id) = self.selected_id.get_untracked()
                && let Some(node_ref) = rt_mut.card_refs.get(&target_id)
            {
                next_focus_target = Some((target_id, *node_ref));
            }
        }

        if remaining.is_empty() {
            let was_focused = self.focused.get_untracked();
            self.focused.set(false);
            self.focus_visible.set(false);
            if was_focused
                && let Some(prev) = restore_node
                && let Some(host) = self.host_handle()
            {
                host.focus(prev);
            }
        } else if let Some((target_id, node_ref)) = next_focus_target {
            self.selected_id.set(Some(target_id));
            node_ref.focus();
        }
    }

    pub(crate) fn dismiss_all(self) {
        let mut drained_timers = Vec::new();
        let mut restore_node = None;

        if let Some(rt) = self.runtime_rc() {
            let mut rt_mut = rt.borrow_mut();
            drained_timers.extend(rt_mut.timers.drain().map(|(_, handle)| handle));
            rt_mut.card_refs.clear();
            rt_mut.slot_ids.clear();
            restore_node = rt_mut.previous_focus.take();
        }
        drop(drained_timers);

        let was_focused = self.focused.get_untracked();
        self.items.update(Vec::clear);
        self.hovered_ids.update(BTreeSet::clear);
        self.selected_id.set(None);
        self.focused.set(false);
        self.focus_visible.set(false);

        if was_focused
            && let Some(prev) = restore_node
            && let Some(host) = self.host_handle()
        {
            host.focus(prev);
        }
    }

    pub(crate) fn items(self) -> RwSignal<Vec<Notification>, LocalStorage> {
        self.items
    }

    pub(crate) fn focused(self) -> RwSignal<bool, LocalStorage> {
        self.focused
    }

    pub(crate) fn focus_visible(self) -> RwSignal<bool, LocalStorage> {
        self.focus_visible
    }

    pub(crate) fn selected_id(self) -> RwSignal<Option<u64>, LocalStorage> {
        self.selected_id
    }

    /// Returns the ID that `Alt+X` would currently dismiss (selected card when focused, or newest
    /// card when unfocused).
    pub(crate) fn target_id(self) -> Option<u64> {
        let items = self.items.get();
        if self.focused.get() {
            self.selected_id
                .get()
                .filter(|id| items.iter().any(|item| item.id == *id))
                .or_else(|| items.first().map(|item| item.id))
        } else {
            items.first().map(|item| item.id)
        }
    }

    pub(crate) fn set_hovered(self, id: u64, hovered: bool) {
        self.hovered_ids.update(|set| {
            if hovered {
                set.insert(id);
            } else {
                set.remove(&id);
            }
        });
        if hovered {
            self.cancel_timer(id);
        } else {
            self.schedule_autohide_if_eligible(id);
        }
    }

    pub(crate) fn mark_card_focused(self, id: u64, cause: FocusCause) {
        self.capture_previous_focus();
        self.selected_id.set(Some(id));
        self.focused.set(true);
        if cause == FocusCause::Pointer {
            self.focus_visible.set(false);
        } else if cause.shows_ring() {
            self.focus_visible.set(true);
        }
        self.pause_all_timers();
    }

    pub(crate) fn mark_pointer_press(self, id: u64) {
        self.selected_id.set(Some(id));
        self.focus_visible.set(false);
    }

    pub(crate) fn clear_focus(self, restore_focus: bool) {
        let mut restore_node = None;
        if let Some(rt) = self.runtime_rc() {
            restore_node = rt.borrow_mut().previous_focus.take();
        }
        self.focused.set(false);
        self.focus_visible.set(false);
        self.resume_unhovered_timers();
        if restore_focus
            && let Some(prev) = restore_node
            && let Some(host) = self.host_handle()
        {
            host.focus(prev);
        }
    }

    pub(crate) fn focus_notification(self, id: u64) {
        self.selected_id.set(Some(id));
        self.focused.set(true);
        self.focus_visible.set(true);
        self.pause_all_timers();
        let card = self
            .runtime_rc()
            .and_then(|rt| rt.borrow().card_refs.get(&id).copied());
        if let Some(card_ref) = card {
            card_ref.focus();
        }
    }

    pub(crate) fn capture_previous_focus(self) {
        let Some(host) = self.host_handle() else {
            return;
        };
        let Some(active) = host.focused().get_untracked() else {
            return;
        };
        let Some(rt) = self.runtime_rc() else {
            return;
        };
        let mut rt_mut = rt.borrow_mut();
        let inside_notifications = rt_mut
            .host_ref
            .as_ref()
            .is_some_and(|r| r.get_untracked() == Some(active) || r.contains(active));
        if !inside_notifications && rt_mut.previous_focus.is_none() {
            rt_mut.previous_focus = Some(active);
        }
    }

    fn cancel_timer(self, id: u64) {
        let removed = self
            .runtime_rc()
            .and_then(|rt| rt.borrow_mut().timers.remove(&id));
        drop(removed);
    }

    fn pause_all_timers(self) {
        let mut drained = Vec::new();
        if let Some(rt) = self.runtime_rc() {
            drained.extend(rt.borrow_mut().timers.drain().map(|(_, handle)| handle));
        }
        drop(drained);
    }

    fn resume_unhovered_timers(self) {
        let candidates: Vec<u64> = self
            .items
            .get_untracked()
            .into_iter()
            .filter(|item| item.autohide)
            .map(|item| item.id)
            .collect();
        for id in candidates {
            self.schedule_autohide_if_eligible(id);
        }
    }

    fn schedule_autohide_if_eligible(self, id: u64) {
        let Some(timers_api) = self.timers_api() else {
            return;
        };
        if self.focused.get_untracked() || self.hovered_ids.get_untracked().contains(&id) {
            return;
        }
        let should_autohide = self
            .items
            .get_untracked()
            .into_iter()
            .find(|item| item.id == id)
            .is_some_and(|item| item.autohide);
        if !should_autohide {
            return;
        }

        self.cancel_timer(id);
        let handle = timers_api.set_timeout(AUTO_DISMISS_DELAY, move || {
            if self.focused.get_untracked() || self.hovered_ids.get_untracked().contains(&id) {
                return;
            }
            self.dismiss(id);
        });
        if let Some(rt) = self.runtime_rc() {
            let old = rt.borrow_mut().timers.insert(id, handle);
            drop(old);
        }
    }

    fn clear_timers(self) {
        let mut drained = Vec::new();
        if let Some(rt) = self.runtime_rc() {
            drained.extend(rt.borrow_mut().timers.drain().map(|(_, handle)| handle));
        }
        drop(drained);
    }
}

fn handle_window_shortcut(queue: NotificationQueue, ev: &mut EventCx<'_, events::KeyDown>) {
    if has_active_modal_scope() {
        return;
    }
    let dispatcher = ActionDispatcher::commands();
    match dispatcher.match_event(ev, ev.modifiers) {
        Some(Command::FocusNotifications) if !queue.items().get_untracked().is_empty() => {
            interaction::focus_in(queue);
            ev.prevent_default();
            ev.stop_propagation();
        }
        Some(Command::CloseNotification) if !queue.items().get_untracked().is_empty() => {
            interaction::close_current_in(queue);
            ev.prevent_default();
            ev.stop_propagation();
        }
        Some(Command::CloseAllNotifications) if !queue.items().get_untracked().is_empty() => {
            interaction::close_all_in(queue);
            ev.prevent_default();
            ev.stop_propagation();
        }
        _ => {}
    }
}

fn handle_stack_key_down(queue: NotificationQueue, ev: &mut EventCx<'_, events::KeyDown>) {
    if ev.repeat {
        return;
    }
    let modifiers = ev.modifiers.held();
    match &ev.key {
        Key::Named(NamedKey::ArrowDown) if modifiers.is_empty() => {
            interaction::select_next_in(queue);
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Named(NamedKey::ArrowUp) if modifiers.is_empty() => {
            interaction::select_prev_in(queue);
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Named(NamedKey::Home) if modifiers.is_empty() => {
            interaction::select_first_in(queue);
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Named(NamedKey::End) if modifiers.is_empty() => {
            interaction::select_last_in(queue);
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Named(NamedKey::Escape) if modifiers.is_empty() => {
            queue.clear_focus(true);
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Named(NamedKey::Delete) if modifiers.is_empty() => {
            interaction::close_current_in(queue);
            ev.prevent_default();
            ev.stop_propagation();
        }
        Key::Character(ch)
            if (modifiers == Modifiers::PRIMARY || modifiers == Modifiers::CONTROL)
                && ch.as_str().eq_ignore_ascii_case("c") =>
        {
            interaction::copy_selected_in(queue);
            ev.prevent_default();
            ev.stop_propagation();
        }
        _ => {}
    }
}

/// Mounts the per-window notification portal stack (`top: 42px; right: 12px`, `Role::Alert` ->
/// AT-SPI `'notification'`, `Alt+N` toggle, `Alt+X` / `Alt+Shift+X` dismiss).
#[component]
pub fn NotificationHost(children: Children) -> impl IntoView {
    if current_host().is_some() {
        zgui::view::install_stylesheet("gds-ui-notifications-host", HOST_SHEET);
        zgui::view::install_stylesheet("gds-ui-notifications-surface", surface::SHEET);
    }

    let queue = NotificationQueue::new();
    provide_local_context(queue);
    let host_id = NEXT_HOST_ID.with(|cell| {
        let next = cell.get().wrapping_add(1);
        cell.set(next);
        next
    });
    MOUNTED_HOSTS.with(|hosts| {
        hosts.borrow_mut().push((host_id, queue));
    });
    let items = queue.items();

    let host_ref = NodeRef::new();
    let column_ref = NodeRef::new();
    queue.register_host_ref(column_ref);

    let has_items = Signal::derive_local(move || !items.get().is_empty());
    register_occlusion_surface(host_ref, column_ref, has_items);
    stage_shortcut(
        column_ref,
        Command::FocusNotifications,
        HintPlacement::Corner,
        has_items,
    );
    stage_shortcut(
        column_ref,
        Command::CloseAllNotifications,
        HintPlacement::BottomLeadingCorner,
        has_items,
    );

    let root_guard: Rc<RefCell<Option<ListenerGuard>>> = Rc::new(RefCell::new(None));
    let watch = {
        let root_guard = Rc::clone(&root_guard);
        RenderEffect::new(move |_| {
            if host_ref.get().is_none() || root_guard.borrow().is_some() {
                return;
            }
            let Some(root) = host_ref.window_root() else {
                return;
            };
            let guard = root.listen(
                events::KEY_DOWN,
                ListenerOptions::CAPTURE,
                move |ev: &mut EventCx<'_, events::KeyDown>| handle_window_shortcut(queue, ev),
            );
            *root_guard.borrow_mut() = guard;
        })
    };

    if let Some(host) = current_host() {
        let host_focus = host.focused();
        let col_check = column_ref;
        let focus_watch = RenderEffect::new(move |_| {
            if !queue.focused().get_untracked() {
                return;
            }
            let still_inside = host_focus.get().is_some_and(|active| {
                col_check.get_untracked() == Some(active) || col_check.contains(active)
            });
            if !still_inside {
                queue.clear_focus(false);
            }
        });
        on_cleanup_local(move || drop(focus_watch));
    }

    on_cleanup_local(move || {
        drop(watch);
        drop(root_guard.borrow_mut().take());
        MOUNTED_HOSTS.with(|hosts| {
            hosts.borrow_mut().retain(|&(id, _)| id != host_id);
        });
        queue.clear_timers();
    });

    view! {
        box(node_ref = host_ref, class = "gds-notification-root") {
            {children.into_view_once()}
            Portal(layer = OverlayLayer::Toast) {
                Show(when = move || !items.get().is_empty()) {
                    column(
                        node_ref = column_ref,
                        class = "gds-notification-column",
                        on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                            handle_stack_key_down(queue, ev);
                        },
                        on:focus_out = move |ev: &mut EventCx<'_, events::FocusOut>| {
                            if ev.related.is_none() {
                                queue.clear_focus(false);
                            }
                        }
                    ) {
                        for entry in move || items.get(), key = |n| n.id {
                            {surface::render_notification(queue, entry)}
                        }
                    }
                }
            }
        }
    }
}

/// Pushes a structured [`Report`] into the active notification stack.
pub fn report(report: Report) {
    NotificationQueue::active().push(report);
}

/// Pushes an informational notification.
pub fn info(message: impl Into<String>) {
    report(Report::new(Level::Info, message));
}

/// Pushes a success notification.
pub fn success(message: impl Into<String>) {
    report(Report::new(Level::Success, message));
}

/// Pushes a warning notification.
pub fn warning(message: impl Into<String>) {
    report(Report::new(Level::Warning, message));
}

/// Pushes an error notification.
pub fn error(message: impl Into<String>) {
    report(Report::new(Level::Error, message));
}
