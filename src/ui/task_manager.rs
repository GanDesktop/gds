//! Read-only task tree backed by the existing task registry.
use crate::{
    app::Gds,
    i18n,
    service::task_manager::{Execution, Inspector, Snapshot, TaskSnapshot},
    ui::Icon,
};
use anti_webui::prelude::*;
use gds_uim::el;
use std::{collections::HashSet, time::Duration};

const ROW_HEIGHT: f32 = 30.0;

#[derive(Clone)]
enum Row {
    Scope(crate::service::task_manager::ScopeSnapshot, usize),
    Task(TaskSnapshot, usize),
}

#[derive(Clone)]
pub struct View {
    snapshot: Signal<Snapshot>,
    collapsed: Signal<HashSet<u64>>,
    selected: Signal<Option<u64>>,
    focus: NodeRef,
}

impl View {
    pub fn new(inspector: Inspector, active: impl Fn() -> bool + 'static, focus: NodeRef) -> Self {
        let snapshot = signal(Snapshot::default());
        let collapsed = signal(HashSet::<u64>::new());
        let selected = signal(None);
        effect(move || {
            if !active() {
                return;
            }
            let inspector = inspector.clone();
            let update = move |next: Snapshot| {
                if let Some(id) = selected.get_untracked()
                    && !next.tasks.iter().any(|task| task.id == id)
                {
                    selected.set(None);
                }
                collapsed
                    .update(|ids| ids.retain(|id| next.scopes.iter().any(|scope| scope.id == *id)));
                snapshot.set(next);
            };
            update(inspector.snapshot());
            let sender = ui_channel(update);
            let scope = Gds::current().tasks().owner_scope("Task inspector");
            scope
                .spawn_cancellable_named("Refresh task inspector", async move {
                    let interval = Duration::from_millis(250);
                    let mut clock =
                        tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
                    loop {
                        clock.tick().await;
                        sender.send(inspector.snapshot());
                    }
                })
                .on_complete(&scope, |result: Result<(), _>| {
                    if let Err(error) = result {
                        tracing::debug!(%error,"task inspector stopped");
                    }
                });
        });
        Self {
            snapshot,
            collapsed,
            selected,
            focus,
        }
    }

    fn rows(&self) -> Vec<Row> {
        fn children(
            snapshot: &Snapshot,
            collapsed: &HashSet<u64>,
            parent: Option<u64>,
            depth: usize,
            rows: &mut Vec<Row>,
        ) {
            for scope in snapshot
                .scopes
                .iter()
                .filter(|scope| scope.parent == parent)
            {
                rows.push(Row::Scope(scope.clone(), depth));
                if collapsed.contains(&scope.id) {
                    continue;
                }
                rows.extend(
                    snapshot
                        .tasks
                        .iter()
                        .filter(|task| task.scope == scope.id)
                        .cloned()
                        .map(|task| Row::Task(task, depth + 1)),
                );
                children(snapshot, collapsed, Some(scope.id), depth + 1, rows);
            }
        }
        let mut rows = Vec::new();
        children(
            &self.snapshot.get(),
            &self.collapsed.get(),
            None,
            0,
            &mut rows,
        );
        rows
    }

    fn row(&self, row: Row) -> Div {
        let selected = self.selected;
        let collapsed = self.collapsed;
        let focus = self.focus;
        let depth = match &row {
            Row::Scope(_, depth) | Row::Task(_, depth) => *depth,
        };
        let base = el! { .div.{
            h: ROW_HEIGHT; w_full; flex; items_center; gap: 8.0_f32; pr: 12.0_f32;
            pl: $(12.0 + depth as f32 * 16.0); text_size: 12.0_f32; cursor: CursorIcon::Pointer;
        } };
        match row {
            Row::Scope(scope, _) => {
                let id = scope.id;
                let state = if scope.cancelling {
                    "logs-tasks-scope-closing"
                } else {
                    "logs-tasks-scope-open"
                };
                el! { base.{
                    dyn_style: s => $(s.bg(theme().surface_muted));
                    on_click: move _ => ${
                        collapsed.update(|ids| { if !ids.remove(&id) { ids.insert(id); } });
                        focus.focus();
                    };
                }.[
                    (move () => $(if collapsed.with(|ids| ids.contains(&id)) {
                        Icon::ChevronRight.view(12.0)
                    } else {
                        Icon::ChevronDown.view(12.0)
                    })).dyn_view,
                    $(Self::name_with_tags(id, scope.name, state, scope.cancelling)),
                ] }
            }
            Row::Task(task, _) => {
                let id = task.id;
                let name = Self::task_name(&task);
                let state = if task.cancelling {
                    "logs-tasks-cancelling"
                } else {
                    "logs-tasks-active"
                };
                let execution = match task.execution {
                    Execution::Supervisor => "logs-tasks-supervisor",
                    Execution::GpuiBackground => "logs-tasks-gpui-background",
                    Execution::Tokio => "logs-tasks-tokio",
                    Execution::Blocking => "logs-tasks-blocking",
                    Execution::Thread => "logs-tasks-thread",
                };
                el! { base.{
                    dyn_style: move s => $(if selected.get() == Some(id) {s.bg(theme().selected)} else {s});
                    on_click: move _ => ${
                        selected.set((selected.get_untracked() != Some(id)).then_some(id));
                        focus.focus();
                    };
                }.[
                    $(Self::name_with_tags(id, name, state, task.cancelling)),
                    (move () => execution.i18n::text).text.{
                        w: 130.0_f32; flex_none; ellipsis; dyn_style: s => $(s.text_color(theme().text_muted));
                    },
                    $(format!("{:.1} s", task.age.as_secs_f64())).text.{
                        w: 75.0_f32; flex_none; dyn_style: s => $(s.text_color(theme().text_muted));
                    },
                ] }
            }
        }
    }

    fn name_with_tags(id: u64, name: String, status: &'static str, cancelling: bool) -> Div {
        let badge = |label| {
            el! { .div.{
                flex; items_center; flex_none; px: 4.0_f32; rounded: 4.0_f32; border: 1.0_f32;
                text_size: 10.0_f32;
                dyn_style: move s => $(if cancelling {
                    s.border_color(theme().warning).text_color(theme().warning_text)
                } else {
                    s.border_color(theme().border_strong).text_color(theme().text_muted)
                });
            }.[label.text] }
        };
        el! { .div.{flex; items_center; flex_1; min_w: 0.0_f32; gap: 4.0_f32}.[
            name.text.{min_w: 0.0_f32; ellipsis},
            format!("#{id}").badge,
            status.i18n::text.badge,
        ] }
    }

    fn task_name(task: &TaskSnapshot) -> String {
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

    fn detail(&self) -> Option<String> {
        let selected = self.selected.get();
        self.snapshot.with(|snapshot| {
            snapshot
                .tasks
                .iter()
                .find(|task| Some(task.id) == selected)
                .map(|task| format!("{} · #{}\n{}", Self::task_name(task), task.id, task.source))
        })
    }
}

impl IntoView for View {
    fn into_view(self) -> anti_webui::prelude::View {
        let rows = signal(Vec::new());
        let observed = self.clone();
        effect(move || rows.set(observed.rows()));
        let snapshot = self.snapshot;
        let detail = self.clone();
        let focus = self.focus;
        let list = virtual_list(
            move || rows.with(Vec::len),
            ROW_HEIGHT,
            move |index| {
                let view = self.clone();
                dyn_view(move || {
                    rows.with(|rows| rows.get(index).cloned())
                        .map_or_else(div, |row| view.row(row))
                })
            },
        );
        el! { .div.{flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32; node_ref: focus; focusable}.[
            .div.{flex; items_center; gap: 8.0_f32; px: 12.0_f32; pt: 12.0_f32; pb: 8.0_f32;
                text_size: 12.0_f32; dyn_style: s => $(s.text_color(theme().text_muted));
            }.[
                (() => "logs-tasks-name".i18n::text).text.{flex_1; min_w: 0.0_f32},
                (() => "logs-tasks-execution".i18n::text).text.{flex_none; w: 130.0_f32},
                (() => "logs-tasks-age".i18n::text).text.{flex_none; w: 75.0_f32},
            ],
            list.{flex_1; min_h: 0.0_f32},
            (move () => $(snapshot.with(|s| s.tasks.is_empty())), () => (() => "logs-tasks-empty".i18n::text).text.{p: 12.0_f32}).show,
            (move () => ${
                el! { .div.[
                    if let Some(description) = $(detail.detail()) [
                        .div.{p: 12.0_f32; max_h: 180.0_f32; overflow_y_scroll; border_t: 1.0_f32;
                            dyn_style: s => $(s.border_color(theme().border));
                        }.[description.text.{select_text; text_size: 12.0_f32}],
                    ],
                ] }
            }).dyn_view,
        ] }.into_view()
    }
}
