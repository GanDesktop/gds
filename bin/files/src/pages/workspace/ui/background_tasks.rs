//! Window-owned running operations shown by the footer and its click popover.
use super::*;
use super::{file_label::FileLabel, status::*};
use crate::entities::file::ui::FileIcon;
use gds::ui::IconButton;
use gds_uim::el;
use gpui_kit::{assets::IconName, prelude::*};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Item {
    key: SharedString,
    label: SharedString,
    cancel: CancellationToken,
}

#[derive(Default)]
pub(super) struct State {
    items: Vec<Item>,
}

impl Drop for State {
    fn drop(&mut self) {
        for item in &self.items {
            item.cancel.cancel();
        }
    }
}

impl State {
    pub(super) fn sync_remote(
        &mut self,
        key: SharedString,
        task: Option<(String, CancellationToken)>,
        cx: &mut Context<Self>,
    ) {
        if let Some((label, cancel)) = task {
            if let Some(item) = self.items.iter_mut().find(|item| item.key == key) {
                item.label = label.into();
                item.cancel = cancel;
            } else {
                self.items.push(Item {
                    key,
                    label: label.into(),
                    cancel,
                });
            }
        } else {
            self.items.retain(|item| item.key != key);
        }
        cx.notify();
    }
    pub fn token(&self, key: &str) -> CancellationToken {
        self.items
            .iter()
            .find(|item| item.key.as_ref() == key)
            .map(|item| item.cancel.clone())
            .unwrap_or_default()
    }
}

impl Render for State {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.items.iter().enumerate().map(|(index, item)| {
            let cancel = item.cancel.clone();
            let cancelled = cancel.is_cancelled();
            let label = if cancelled {
                format!("{} — {}", item.label, i18n::text("tasks-cancelling", cx)).into()
            } else {
                item.label.clone()
            };
            let on_click = cx.listener(move |_, _, _, cx| {
                cancel.cancel();
                cx.notify();
            });
            el! {
                .StatusRow::default {
                    # $(item.key.clone()); title: ~label;
                    content: .FileLabel::default {
                        icon: $(FileIcon::plain(IconName::ListTodo)); name: label;
                    };
                    action: ($("cancel-task", index)).IconButton::new {
                        icon: IconName::X; label: $(i18n::text("operation-cancel", cx));
                        disabled: cancelled; on_click: on_click;
                    };
                }
            }
        });
        el! { .StatusList::default { empty: $(i18n::text("tasks-empty", cx)); } [..rows] }
    }
}

impl Browser {
    pub(super) fn start_background(&mut self, label: SharedString, cx: &mut Context<Self>) {
        self.start_background_kind("files", label, cx);
    }
    pub(super) fn start_background_kind(
        &mut self,
        key: &'static str,
        label: SharedString,
        cx: &mut Context<Self>,
    ) {
        let cancel = self.window.tasks.token().child_token();
        self.background_tasks.update(cx, |state, cx| {
            if let Some(item) = state.items.iter_mut().find(|item| item.key.as_ref() == key) {
                item.label = label;
            } else {
                state.items.push(Item {
                    key: key.into(),
                    label,
                    cancel,
                });
            }
            cx.notify();
        });
        cx.notify();
    }
    pub(super) fn finish_background(
        &mut self,
        report: notifications::Report,
        cx: &mut Context<Self>,
    ) {
        self.finish_background_kind("files", report, cx);
    }
    pub(super) fn finish_background_kind(
        &mut self,
        key: &str,
        report: notifications::Report,
        cx: &mut Context<Self>,
    ) {
        self.background_tasks.update(cx, |state, cx| {
            state.items.retain(|item| item.key.as_ref() != key);
            cx.notify();
        });
        report.show(cx);
        cx.notify();
    }
    pub(super) fn render_background_tasks(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.background_tasks.clone();
        let count = state.read(cx).items.len();
        let label = match count {
            0 => SharedString::default(),
            1 => state.read(cx).items[0].label.clone(),
            _ => i18n::count("tasks-count", count, cx),
        };
        el! {
            .StatusControl::default {
                # "background-tasks"; title: $(i18n::text("tasks-title", cx));
                summary: .StatusSummary::default {
                    compact: $(self.window.layout.compact); icon: IconName::ListTodo;
                    label: label; count: count;
                };
                details: state;
            }
        }
        .into_any_element()
    }
}
