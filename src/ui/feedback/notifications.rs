//! Window-owned, readable toast stack; failures remain until explicitly dismissed.
use crate::{
    appearance::tokens,
    ui::{Icon, IconButton},
};
use anti_webui::core::access::{self, AccessAction};
use anti_webui::prelude::*;
use gds_uim::el;
use std::time::Duration;

const ENTER: Transition = Transition::ms(400).cubic_bezier(0.25, 0.1, 0.25, 1.0);
const EXIT: Transition = Transition::ms(200).cubic_bezier(0.25, 0.1, 0.25, 1.0);
const TRAVEL: f32 = 96.0;

#[derive(Clone, Debug)]
pub enum Notice {
    Info(String),
    Success(String),
    Error(String),
}
impl Notice {
    fn message(&self) -> &str {
        match self {
            Self::Info(message) | Self::Success(message) | Self::Error(message) => message,
        }
    }
    fn icon(&self) -> Icon {
        match self {
            Self::Info(_) => Icon::Info,
            Self::Success(_) => Icon::CircleCheck,
            Self::Error(_) => Icon::CircleClose,
        }
    }
    fn color(&self) -> Color {
        match self {
            Self::Info(_) => theme().accent,
            Self::Success(_) => theme().success_text,
            Self::Error(_) => theme().danger_text,
        }
    }
}
#[derive(Clone)]
struct Entry {
    id: u64,
    notice: Notice,
    hovered: Signal<bool>,
    closing: Signal<bool>,
    opacity: Animated<f32>,
    node: NodeRef,
}

#[derive(Clone, Copy)]
struct ReturnFocus {
    previous: Option<u64>,
    fallback: NodeRef,
}

#[derive(Clone, Copy)]
pub struct Center {
    entries: Signal<Vec<Entry>>,
    next: Signal<u64>,
    current: Signal<Option<u64>>,
    focused: Signal<bool>,
    focus_visible: Signal<bool>,
    node: NodeRef,
    restore: Signal<Option<ReturnFocus>>,
    owner: Owner,
}
impl Default for Center {
    fn default() -> Self {
        Self::new()
    }
}
impl Center {
    pub fn new() -> Self {
        Self {
            entries: signal(Vec::new()),
            next: signal(0),
            current: signal(None),
            focused: signal(false),
            focus_visible: signal(false),
            node: node_ref(),
            restore: signal(None),
            owner: Owner::current().expect("notification window owner"),
        }
    }
    pub fn push(self, notice: Notice) -> u64 {
        let id = self.next.get_untracked();
        self.next.set(id.wrapping_add(1));
        let persistent = matches!(notice, Notice::Error(_));
        self.owner.with(|| {
            let hovered = signal(false);
            let opacity = animated(0.0_f32);
            let closing = signal(false);
            let node = node_ref();
            self.entries.update(|entries| {
                entries.push(Entry {
                    id,
                    notice,
                    hovered,
                    closing,
                    opacity,
                    node,
                })
            });
            opacity.to(1.0, ENTER);
            if !persistent {
                self.expire(id);
            }
        });
        if !self.focused.get_untracked() {
            self.current.set(Some(id));
        }
        id
    }
    fn expire(self, id: u64) {
        self.owner.with(|| {
            anti_webui::core::animation::set_timeout(Duration::from_secs(5), move || {
                let held = self.entries.with_untracked(|entries| {
                    entries
                        .iter()
                        .find(|entry| entry.id == id)
                        .map(|entry| entry.hovered.get_untracked())
                });
                match held {
                    Some(true) => self.expire(id),
                    Some(false)
                        if self.focused.get_untracked()
                            && self.current.get_untracked() == Some(id) =>
                    {
                        self.expire(id)
                    }
                    Some(false) => self.dismiss(id),
                    None => {}
                }
            });
        });
    }
    pub fn notify(self, message: impl Into<String>) -> u64 {
        self.push(Notice::Info(message.into()))
    }
    pub fn success(self, message: impl Into<String>) -> u64 {
        self.push(Notice::Success(message.into()))
    }
    pub fn error(self, message: impl Into<String>) -> u64 {
        self.push(Notice::Error(message.into()))
    }
    pub fn dismiss(self, id: u64) {
        let entry = self
            .entries
            .with_untracked(|entries| entries.iter().find(|entry| entry.id == id).cloned());
        let Some(entry) = entry.filter(|entry| !entry.closing.get_untracked()) else {
            return;
        };
        entry.closing.set(true);
        entry.opacity.to(0.0, EXIT);
        if self.current.get_untracked() == Some(id) {
            let next = self.entries.with_untracked(|entries| {
                let index = entries.iter().position(|entry| entry.id == id)?;
                entries[index..]
                    .iter()
                    .chain(entries[..index].iter().rev())
                    .find(|entry| !entry.closing.get_untracked())
                    .map(|entry| entry.id)
            });
            self.current.set(next);
            if next.is_some() {
                if self.focused.get_untracked() {
                    self.node.focus();
                }
                self.reveal_current();
            } else {
                self.restore_focus();
            }
        }
        let remove = move || {
            self.entries
                .update(|entries| entries.retain(|entry| entry.id != id))
        };
        if crate::appearance::motion::enabled() {
            self.owner.with(|| {
                anti_webui::core::animation::set_timeout(Duration::from_millis(200), remove)
            });
        } else {
            remove();
        }
    }
    pub fn dismiss_current(self) {
        let target = if self.focused.get_untracked() {
            self.current.get_untracked()
        } else {
            self.latest()
        };
        if let Some(id) = target {
            self.dismiss(id);
        }
    }
    pub fn clear(self) {
        let ids = self
            .entries
            .with_untracked(|entries| entries.iter().map(|entry| entry.id).collect::<Vec<_>>());
        self.restore_focus();
        self.current.set(None);
        for id in ids {
            self.dismiss(id);
        }
    }
    fn latest(self) -> Option<u64> {
        self.entries.with_untracked(|entries| {
            entries
                .iter()
                .rev()
                .find(|entry| !entry.closing.get_untracked())
                .map(|entry| entry.id)
        })
    }
    fn reveal_current(self) {
        let id = self.current.get_untracked();
        if let Some(node) = self.entries.with_untracked(|entries| {
            entries
                .iter()
                .find(|entry| Some(entry.id) == id)
                .map(|entry| entry.node)
        }) {
            node.scroll_into_view();
        }
    }
    fn restore_focus(self) {
        let target = self.restore.update(Option::take);
        if !self.focused.get_untracked() {
            return;
        }
        self.focused.set(false);
        self.focus_visible.set(false);
        let Some(target) = target else {
            return;
        };
        let restored = target.previous.is_some_and(|id| {
            let tree = access::snapshot();
            tree.get(id)
                .is_some_and(|node| node.focusable && !node.disabled)
                && access::perform(id, AccessAction::Focus)
        });
        if !restored {
            target.fallback.focus();
        }
    }
    pub fn focus_current(self, fallback: NodeRef) {
        if self.focused.get_untracked() {
            if self.restore.get_untracked().is_some() {
                self.restore_focus();
            }
        } else if let Some(id) = self.latest() {
            self.restore.set(Some(ReturnFocus {
                previous: access::snapshot().focus,
                fallback,
            }));
            self.current.set(Some(id));
            self.node.focus();
            self.reveal_current();
        }
    }
    fn control_focused(self, id: u64, focused: bool) {
        self.focused.set(focused);
        if focused {
            self.current.set(Some(id));
        }
    }
    fn key(self, event: &KeyboardEvent) {
        match &event.key {
            Key::Named(
                NamedKey::ArrowDown | NamedKey::ArrowUp | NamedKey::Home | NamedKey::End,
            ) if event.modifiers == Modifiers::default() => {
                let next = self.entries.with_untracked(|entries| {
                    let ids = entries
                        .iter()
                        .filter(|entry| !entry.closing.get_untracked())
                        .map(|entry| entry.id)
                        .collect::<Vec<_>>();
                    let last = ids.len().checked_sub(1)?;
                    let current = self.current.get_untracked();
                    let index = ids
                        .iter()
                        .position(|&id| Some(id) == current)
                        .unwrap_or(last);
                    let next = match event.key {
                        Key::Named(NamedKey::ArrowDown) => (index + 1).min(last),
                        Key::Named(NamedKey::ArrowUp) => index.saturating_sub(1),
                        Key::Named(NamedKey::Home) => 0,
                        _ => last,
                    };
                    Some(ids[next])
                });
                self.current.set(next);
                self.reveal_current();
            }
            Key::Named(NamedKey::Escape) if self.restore.get_untracked().is_some() => {
                self.restore_focus()
            }
            Key::Character(value)
                if event.modifiers.alt
                    && !event.modifiers.ctrl
                    && !event.modifiers.logo
                    && !event.modifiers.shift
                    && value.eq_ignore_ascii_case("x") =>
            {
                if let Some(id) = self.current.get_untracked().or_else(|| self.latest()) {
                    self.dismiss(id);
                }
            }
            Key::Character(value)
                if event.modifiers.alt
                    && !event.modifiers.ctrl
                    && !event.modifiers.logo
                    && !event.modifiers.shift
                    && value.eq_ignore_ascii_case("n")
                    && self.restore.get_untracked().is_some() =>
            {
                self.restore_focus();
            }
            Key::Character(value) if event.modifiers.ctrl && value.eq_ignore_ascii_case("c") => {
                if let Some(message) = self.entries.with_untracked(|entries| {
                    entries
                        .iter()
                        .find(|entry| Some(entry.id) == self.current.get_untracked())
                        .map(|entry| entry.notice.message().to_owned())
                }) {
                    set_clipboard(ClipboardKind::Clipboard, message);
                }
            }
            _ => return,
        }
        event.prevent_default();
        event.stop_propagation();
    }
    pub fn view(self) -> Div {
        el! { .div.{
            absolute; right:tokens::NOTIFICATION_INSET; top:$(tokens::TOOLBAR_HEIGHT+tokens::NOTIFICATION_TOP_GAP);
            flex; flex_col; gap:8.0_f32; w:382.0_f32; max_w:80.0_f32.pct; max_h:80.0_f32.pct;
            overflow_y_scroll; focusable; min_h:0.0_f32; node_ref: self->node;
            on_focus_change:move focused=>self->focused.{set:focused};
            on_focus_visible:move focused=>self->focus_visible.{set:focused};
            on_key_down:move event=>self.{key:event};
        }.[
            (move()=>self->entries.{get},entry=>entry->id,move entry=>${
                let id=entry.id;
                let message=entry.notice.message().to_owned();
                let copy=message.clone();
                let notice=entry.notice;
                let icon=notice.icon();
                let hovered=entry.hovered;
                let close_focused = signal(false);
                el! { .div.{
                    relative; flex; items_start; gap:8.0_f32; px:16.0_f32; py:14.0_f32;
                    rounded:8.0_f32; border:1.0_f32; flex_none; node_ref: entry->node;
                    on_hover:move inside=>hovered.{set_if_changed:inside};
                    on_pointer_down:move _=>self->current.{set:$(Some(id))};
                    dyn_style:move s=>${
                        let opacity = entry.opacity.get();
                        let s = s.bg(theme().overlay).border_color(theme().border).text_color(theme().text)
                            .outline(if self.focus_visible.get() && self.current.get() == Some(id) { 1.0_f32 } else { 0.0_f32 })
                            .outline_offset(-1.0_f32).outline_color(theme().focus_ring)
                            .opacity(opacity).translate_y(-TRAVEL * (1.0 - opacity));
                        if entry.closing.get() {s.pointer_events_none()} else {s}
                    };
                    access:a=>$(a.live(Live::Polite));
                }.[
                    .div.{pt:4.0_f32;dyn_style:move s=>$(s.text_color(notice.color()))}.[icon],
                    .div.{flex_1;min_w:0.0_f32;max_h:240.0_f32;overflow_y_scroll}.[message.text],
                    .div.{flex;flex_none;ml:4.0_f32;mr:14.0_f32}.[
                        "copy-notice".IconButton::new.{small;icon:Icon::Copy;label_fn:()=>"notification-copy".crate::i18n::text;
                            on_focus_change:move focused=>self.{control_focused:id,focused};
                            on_click:move _=>$(set_clipboard(ClipboardKind::Clipboard,copy.clone()));
                        },
                    ],
                    .div.{absolute;top:4.0_f32;right:4.0_f32;
                        dyn_style:move s=>$(if hovered.get() || close_focused.get() {s.flex()} else {s.hidden()});
                    }.[
                        "dismiss-notice".IconButton::new.{extra_small;icon:Icon::Close;label_fn:()=>"notification-close".crate::i18n::text;
                            on_focus_change:move focused=>${close_focused.set(focused); self.control_focused(id, focused);};
                            on_click:move _=>self.{dismiss:id};
                        },
                    ],
                    $(super::super::keytips::Anchor::new(
                        super::super::actions::Command::CloseNotification.default_stroke(),
                    ).when(move || self.current.get() == Some(id) && !entry.closing.get())),
                ] }
            }).for_each,
        ] }
    }
}
