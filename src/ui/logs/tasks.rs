//! Live, read-only view of the application's registered tasks.
use super::*;
use crate::task_manager::{Execution, Scope, Snapshot as TaskSnapshot};
use gpui_kit::component::{
    Sizable,
    tag::{Tag as InlineTag, TagVariant},
};
use std::collections::HashSet;

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

pub(super) const REFRESH: Duration = Duration::from_millis(250);
const HEIGHT: f32 = 30.;

enum Row {
    Scope(usize, usize),
    Task(usize, usize),
}

pub(super) struct View {
    scope: Scope,
    snapshot: TaskSnapshot,
    rows: Vec<Row>,
    collapsed: HashSet<u64>,
    selected: Option<u64>,
    scroll: UniformListScrollHandle,
    pub(super) focus: FocusHandle,
    updates: Option<Task<()>>,
}

impl View {
    pub(super) fn new(scope: Scope, cx: &mut Context<Self>) -> Self {
        Self {
            scope,
            snapshot: TaskSnapshot::default(),
            rows: Vec::new(),
            collapsed: HashSet::new(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            updates: None,
        }
    }

    pub(super) fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        self.updates = None;
        if active {
            self.refresh();
            if let Some(index) = self.rows.iter().position(|row| {
                matches!(row,
                Row::Task(index, _) if Some(self.snapshot.tasks[*index].id) == self.selected)
            }) {
                self.scroll
                    .scroll_to_item(index, gpui_kit::ScrollStrategy::Nearest);
            }
            self.updates = Some(cx.spawn(async move |view, cx| {
                loop {
                    cx.background_executor().timer(REFRESH).await;
                    if view
                        .update(cx, |view, cx| {
                            view.refresh();
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        cx.notify();
    }

    fn refresh(&mut self) {
        self.snapshot = self.scope.snapshot();
        self.collapsed
            .retain(|id| self.snapshot.scopes.iter().any(|scope| scope.id == *id));
        if !self
            .snapshot
            .tasks
            .iter()
            .any(|task| Some(task.id) == self.selected)
        {
            self.selected = None;
        }
        self.rebuild();
    }

    fn rebuild(&mut self) {
        fn visit(view: &mut View, parent: Option<u64>, depth: usize) {
            let children: Vec<_> = view
                .snapshot
                .scopes
                .iter()
                .enumerate()
                .filter(|(_, scope)| scope.parent == parent)
                .map(|(index, scope)| (index, scope.id))
                .collect();
            for (index, id) in children {
                view.rows.push(Row::Scope(index, depth));
                if view.collapsed.contains(&id) {
                    continue;
                }
                for (index, task) in view.snapshot.tasks.iter().enumerate() {
                    if task.scope == id {
                        view.rows.push(Row::Task(index, depth + 1));
                    }
                }
                visit(view, Some(id), depth + 1);
            }
        }
        self.rows.clear();
        visit(self, None, 0);
    }

    fn row(&self, row_index: usize, cx: &mut Context<Self>) -> AnyElement {
        let base = div()
            .h(px(HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .pr_3()
            .text_size(px(12.));
        match self.rows[row_index] {
            Row::Scope(index, depth) => {
                let scope = &self.snapshot.scopes[index];
                let id = scope.id;
                let collapsed = self.collapsed.contains(&id);
                let status = if scope.cancelling {
                    t!(@cx, "logs-tasks-scope-closing")
                } else {
                    t!(@cx, "logs-tasks-scope-open")
                };
                base.id(("task-scope", id))
                    .test_support()
                    .pl(px(12. + depth as f32 * 16.))
                    .bg(cx.theme().muted)
                    .cursor_pointer()
                    .child(div().w(px(12.)).flex_shrink_0().child(if collapsed {
                        "▸"
                    } else {
                        "▾"
                    }))
                    .child(name_with_tags(
                        id,
                        scope.name.clone(),
                        status,
                        if scope.cancelling {
                            TagVariant::Warning
                        } else {
                            TagVariant::Secondary
                        },
                    ))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.focus.focus(window, cx);
                        if !this.collapsed.remove(&id) {
                            this.collapsed.insert(id);
                        }
                        this.rebuild();
                        cx.notify();
                    }))
                    .into_any_element()
            }
            Row::Task(index, depth) => {
                let task = &self.snapshot.tasks[index];
                let id = task.id;
                let status = if task.cancelling {
                    t!(@cx, "logs-tasks-cancelling")
                } else {
                    t!(@cx, "logs-tasks-active")
                };
                let variant = if task.cancelling {
                    TagVariant::Warning
                } else {
                    TagVariant::Success
                };
                base.id(("task-entry", id))
                    .aria_label(format!(
                        "{}, {}, {status}",
                        task_name(task),
                        execution(task.execution, cx)
                    ))
                    .test_support()
                    .pl(px(12. + depth as f32 * 16.))
                    .cursor_pointer()
                    .when(self.selected == Some(id), |row| row.bg(cx.theme().accent))
                    .child(name_with_tags(id, task_name(task), status, variant))
                    .child(
                        div()
                            .w(px(130.))
                            .flex_shrink_0()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(execution(task.execution, cx)),
                    )
                    .child(
                        div()
                            .w(px(75.))
                            .flex_shrink_0()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{:.1} s", task.age.as_secs_f64())),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.focus.focus(window, cx);
                        this.selected = (this.selected != Some(id)).then_some(id);
                        this.scroll
                            .scroll_to_item(row_index, gpui_kit::ScrollStrategy::Nearest);
                        cx.notify();
                    }))
                    .into_any_element()
            }
        }
    }
}

fn name_with_tags(
    id: u64,
    name: String,
    status: gpui_kit::SharedString,
    variant: TagVariant,
) -> AnyElement {
    let tag = |label: gpui_kit::SharedString, variant| {
        InlineTag::new()
            .with_variant(variant)
            .small()
            .outline()
            .flex_shrink_0()
            .text_size(px(10.))
            .child(label)
    };
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap_1()
        .child(div().min_w_0().truncate().child(name))
        .child(tag(format!("#{id}").into(), TagVariant::Secondary))
        .child(tag(status, variant))
        .into_any_element()
}

fn execution(kind: Execution, cx: &App) -> gpui_kit::SharedString {
    crate::i18n::text(
        match kind {
            Execution::Supervisor => "logs-tasks-supervisor",
            Execution::GpuiBackground => "logs-tasks-gpui-background",
            Execution::Tokio => "logs-tasks-tokio",
            Execution::Blocking => "logs-tasks-blocking",
            Execution::Thread => "logs-tasks-thread",
        },
        cx,
    )
}

fn task_name(task: &crate::task_manager::TaskSnapshot) -> String {
    if task.name.is_empty() {
        std::path::Path::new(task.source.file())
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        task.name.clone()
    }
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let detail = self
            .snapshot
            .tasks
            .iter()
            .find(|task| Some(task.id) == self.selected);
        div()
            .id("tasks-panel")
            .test_support()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .key_context("GdsTasks")
            .track_focus(&self.focus)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().flex_1().child(t!(@cx, "logs-tasks-name")))
                    .child(
                        div()
                            .w(px(130.))
                            .flex_shrink_0()
                            .child(t!(@cx, "logs-tasks-execution")),
                    )
                    .child(
                        div()
                            .w(px(75.))
                            .flex_shrink_0()
                            .child(t!(@cx, "logs-tasks-age")),
                    ),
            )
            .child(
                uniform_list(
                    "tasks-list",
                    self.rows.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range.map(|index| this.row(index, cx)).collect()
                    }),
                )
                .track_scroll(&self.scroll)
                .flex_1()
                .min_h_0(),
            )
            .when(self.snapshot.tasks.is_empty(), |view| {
                view.child(
                    div()
                        .id("logs-tasks-empty")
                        .test_support()
                        .p_3()
                        .text_sm()
                        .child(t!(@cx, "logs-tasks-empty")),
                )
            })
            .children(detail.map(|task| {
                div()
                    .id("tasks-detail")
                    .test_support()
                    .p_3()
                    .flex_shrink_0()
                    .max_h(px(DETAIL_HEIGHT))
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .child(crate::ui::tooltip::wrap_tokens(&format!(
                        "{} · #{}\n{}",
                        task_name(task),
                        task.id,
                        task.source
                    )))
            }))
    }
}
