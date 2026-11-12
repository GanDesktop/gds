//! An independent Debug window for logs and registered application tasks.
use crate::app::AppExt as _;

use crate::i18n::t;
use crate::service::logs::{Entry, Level, Snapshot, Store};
use crate::ui::window::TitleBar;
use gds_uim::el;
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, FocusHandle, Render, Subscription,
    Task, TestSupportExt, UniformListScrollHandle, Window, actions,
    assets::IconName,
    component::{
        ActiveTheme, Root,
        input::{InputEvent, InputState},
    },
    div,
    prelude::*,
    px, size, uniform_list,
};
use std::time::Duration;

mod filters;
mod navigation;
mod window;

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

const PREVIEW_CHARS: usize = 240;
const ROW_HEIGHT: f32 = 22.;
const DETAIL_HEIGHT: f32 = 180.;
const UPDATE_INTERVAL: Duration = Duration::from_millis(50);

actions!(
    logs,
    [
        CloseLogs,
        PreviousLog,
        NextLog,
        PreviousLogPage,
        NextLogPage
    ]
);

pub use window::{OpenLogs, Options};
pub(crate) use window::{init, install, is_enabled, open, store};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Logs,
    Tasks,
}

pub struct Viewer {
    tab: Tab,
    tasks: Option<Entity<crate::ui::task_manager::View>>,
    store: Store,
    snapshot: Snapshot,
    visible: Vec<usize>,
    selected: Option<u64>,
    level: Option<Level>,
    target: Option<String>,
    query: String,
    scroll: UniformListScrollHandle,
    list_focus: FocusHandle,
    search: Entity<InputState>,
    _search: Subscription,
    _updates: Task<()>,
}

fn icon_action(
    id: impl Into<gpui_kit::ElementId>,
    icon: IconName,
    label: impl Into<gpui_kit::SharedString>,
) -> crate::ui::button::Button {
    let label = label.into();
    crate::ui::button::Button::icon_button(id)
        .control_icon(icon)
        .accessibility_label(label.clone())
        .tooltip(label)
}

impl Viewer {
    pub fn new(store: Store, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Subscribe before the snapshot so a concurrent record cannot be missed.
        let mut changes = store.subscribe();
        let snapshot = store.snapshot();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(t!(@cx, "logs-search")));
        let subscription = cx.subscribe(&search, |this, input, _: &InputEvent, cx| {
            this.query = input.read(cx).value().to_string().to_lowercase();
            this.filter();
            cx.notify();
        });
        let updates = cx.spawn(async move |view, cx| {
            while changes.recv().await.is_some() {
                // Event-driven batching yields even when producers never become idle.
                cx.background_executor().timer(UPDATE_INTERVAL).await;
                while changes.try_recv().is_ok() {}
                if view
                    .update(cx, |this, cx| {
                        this.refresh();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let tasks = cx
            .gds()
            .tasks()
            .inspector()
            .map(|inspector| cx.new(|cx| crate::ui::task_manager::View::new(inspector, cx)));
        let mut viewer = Self {
            tab: Tab::Logs,
            tasks,
            store,
            snapshot,
            visible: Vec::new(),
            selected: None,
            level: None,
            target: None,
            query: String::new(),
            scroll: UniformListScrollHandle::new(),
            list_focus: cx.focus_handle(),
            search,
            _search: subscription,
            _updates: updates,
        };
        viewer.filter();
        viewer.scroll.scroll_to_bottom();
        viewer
    }

    fn filter(&mut self) {
        self.visible = self
            .snapshot
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                self.level.is_none_or(|level| level == entry.level)
                    && self
                        .target
                        .as_ref()
                        .is_none_or(|target| target == &entry.target)
                    && (self.query.is_empty()
                        || entry.message.to_lowercase().contains(&self.query)
                        || entry.target.to_lowercase().contains(&self.query))
            })
            .map(|(index, _)| index)
            .collect();
        if !self
            .visible
            .iter()
            .any(|&index| Some(self.snapshot.entries[index].id) == self.selected)
        {
            self.selected = None;
        }
    }

    fn refresh(&mut self) {
        let snapshot = self.store.snapshot();
        if snapshot.revision != self.snapshot.revision {
            let follow = self.scroll.is_scrolled_to_end().unwrap_or(true);
            self.snapshot = snapshot;
            self.filter();
            if follow {
                self.scroll.scroll_to_bottom();
            }
        }
    }

    fn clear(&mut self) {
        self.store.clear();
        self.refresh();
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.snapshot
            .entries
            .iter()
            .find(|entry| Some(entry.id) == self.selected)
    }

    fn copy_selected(&self, cx: &mut App) {
        if let Some(entry) = self.selected_entry() {
            cx.write_to_clipboard(ClipboardItem::new_string(entry.text()));
        }
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let entry = &self.snapshot.entries[self.visible[index]];
        let id = entry.id;
        let at: chrono::DateTime<chrono::Utc> = entry.time.into();
        let color = match entry.level {
            Level::Error => cx.theme().danger,
            Level::Warn => cx.theme().warning,
            _ => cx.theme().foreground,
        };
        div()
            .id(("log-entry", id))
            .test_support()
            .h(px(ROW_HEIGHT))
            .w_full()
            .px_2()
            .overflow_hidden()
            .whitespace_nowrap()
            .cursor_pointer()
            .flex()
            .items_center()
            .gap_2()
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(px(12.))
            .when(self.selected == Some(id), |row| row.bg(cx.theme().accent))
            .child(
                div()
                    .w(px(100.))
                    .flex_shrink_0()
                    .text_color(cx.theme().muted_foreground)
                    .child(at.format("%H:%M:%S%.3fZ").to_string()),
            )
            .child(
                div()
                    .w(px(40.))
                    .flex_shrink_0()
                    .text_color(color)
                    .child(entry.level.as_str()),
            )
            .child(
                div()
                    .w(px(200.))
                    .flex_shrink_0()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(entry.target.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(preview(&entry.message)),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.list_focus.focus(window, cx);
                this.selected = (this.selected != Some(id)).then_some(id);
                cx.notify();
            }))
            .into_any_element()
    }
}

fn preview(text: &str) -> String {
    let mut chars = text.chars();
    let mut preview: String = chars
        .by_ref()
        .take(PREVIEW_CHARS)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if chars.next().is_some() {
        preview.push('…');
    }
    preview
}

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        el! {
            .div {
                # "logs-window";
                test_support;
                key_context: "GdsLogs";
                size_full;
                flex;
                flex_col;
                bg: $(cx.gds_ro().appearance().theme().content_background());
                text_color: $(cx.theme().foreground);
                on_action: $(cx.listener(|_, _: &CloseLogs, window, _| window.remove_window()));
                on_action: $(cx.listener(|this, _: &PreviousLog, _, cx| this.move_selection(false, false, cx)));
                on_action: $(cx.listener(|this, _: &NextLog, _, cx| this.move_selection(true, false, cx)));
                on_action: $(cx.listener(|this, _: &PreviousLogPage, _, cx| this.move_selection(false, true, cx)));
                on_action: $(cx.listener(|this, _: &NextLogPage, _, cx| this.move_selection(true, true, cx)));
            } [
                .TitleBar::default {
                    on_close_window: $(|_, window, _| window.remove_window());
                } [
                    t!(@cx, "logs-title"),
                ],
                $(gpui_kit::component::tab::TabBar::new("logs-tabs")
                    .underline().px_3().flex_shrink_0()
                    .selected_index(if self.tab == Tab::Logs { 0 } else { 1 })
                    .child(gpui_kit::component::tab::Tab::new().label(t!(@cx, "logs-tab-logs")))
                    .when(self.tasks.is_some(), |tabs| tabs.child(gpui_kit::component::tab::Tab::new().label(t!(@cx, "logs-tab-tasks"))))
                    .on_click(cx.listener(|this, index: &usize, window, cx| {
                        this.switch_tab(if *index == 0 { Tab::Logs } else { Tab::Tasks }, window, cx);
                    }))),
                $(match (&self.tab, &self.tasks) { (Tab::Tasks, Some(tasks)) => tasks.clone().into_any_element(), _ => self.log_content(cx) }),
            ]
        }
    }
}

impl Viewer {
    fn switch_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if tab == Tab::Tasks && self.tasks.is_none() {
            return;
        }
        self.tab = tab;
        if let Some(tasks) = &self.tasks {
            tasks.update(cx, |view, cx| view.set_active(tab == Tab::Tasks, cx));
        }
        self.focus_tab(window, cx);
        cx.notify();
    }

    fn focus_tab(&self, window: &mut Window, cx: &mut App) {
        if self.tab == Tab::Logs {
            self.search
                .update(cx, |search, cx| search.focus(window, cx));
        } else if let Some(tasks) = &self.tasks {
            tasks.read(cx).focus.clone().focus(window, cx);
        }
    }

    fn log_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let detail = self.selected_entry().map(Entry::text);
        el! {
            .div {
                flex;
                flex_col;
                flex_1;
                min_h_0;
                min_w_0;
            } [
                .div {
                    # "logs-toolbar";
                    test_support;
                    flex;
                    items_center;
                    gap_2;
                    p_2;
                    flex_shrink_0;
                } [
                    $(self.level_filter(cx)),
                    $(self.target_filter(cx)),
                    .div {
                        flex_1;
                        min_w_0;
                    } [
                        $(crate::ui::field::placeholder(&self.search, crate::ui::field::input(&self.search, cx).cleanable(true))),
                    ],
                    ("logs-clear", IconName::ListX, t!(@cx, "logs-clear")).icon_action {
                        on_click: $(cx.listener(|this, _, _, cx| { this.clear(); cx.notify(); }));
                    },
                ],
                .div {
                    # "logs-list-viewport";
                    test_support;
                    key_context: "GdsLogsList";
                    track_focus: $(&self.list_focus);
                    flex_1;
                    min_h_0;
                    on_mouse_down: gpui_kit::MouseButton::Left, $(cx.listener(|this, _, window, cx| this.list_focus.focus(window, cx)));
                } [
                    $(uniform_list("logs-list", self.visible.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                    range.map(|index| this.row(index, cx)).collect()
                        })).track_scroll(&self.scroll).size_full()),
                ],
                ..$(detail.map(|text| div().id("logs-detail").test_support().w_full().min_w_0().max_h(px(DETAIL_HEIGHT)).flex_shrink_0().flex().gap_2().p_2()
                        .border_t_1().border_color(cx.theme().border)
                        .child(div().id("logs-detail-text").flex_1().min_w_0().max_h(px(DETAIL_HEIGHT - 16.)).overflow_y_scroll()
                            .font_family(cx.theme().mono_font_family.clone()).text_size(px(12.))
                            .whitespace_normal().child(crate::ui::tooltip::wrap_tokens(&text)))
                        .child(icon_action("logs-copy-selected", IconName::Copy, t!(@cx, "logs-copy-selected"))
                            .on_click(cx.listener(|this, _, _, cx| this.copy_selected(cx))))
                        .child(icon_action("logs-close-detail", IconName::Close, t!(@cx, "logs-close-detail"))
                            .on_click(cx.listener(|this, _, _, cx| { this.selected = None; cx.notify(); }))))),
                .div {
                    px_2;
                    text_xs;
                    text_color: $(cx.theme().muted_foreground);
                } [
                    t!(@cx, "logs-count", visible = self.visible.len(), retained = self.snapshot.entries.len(), evicted = self.snapshot.evicted),
                ],
            ]
        }.into_any_element()
    }
}
