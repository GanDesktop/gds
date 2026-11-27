//! Live, read-only view of the application's registered task scopes and tasks.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Memo, RwSignal, Signal, StoredValue, on_cleanup_local};
use zgui::view::install_stylesheet;
use zgui::view::time::{IntervalHandle, set_interval};
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::app::Gds;
use crate::i18n;
use crate::service::task_manager::{
    Execution, Snapshot as TaskSnapshot, TaskSnapshot as TaskRecord,
};
use crate::ui::icon::{IconName, IconProps};
use crate::ui::tooltip::wrap_tokens;

pub(crate) const REFRESH: Duration = Duration::from_millis(250);
const ROW_HEIGHT: f32 = 30.0;
const SHEET: &str = "gds-task-manager";

const TASK_MANAGER_CSS: &str = "
.gds-tasks-root {
    display: flex;
    flex-direction: column;
    flex: 1 1 0;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    background: transparent;
}
.gds-tasks-header {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 12px;
    background: transparent;
    color: var(--gds-muted-foreground);
    font-size: var(--gds-text-xs, 12px);
}
.gds-tasks-col-name {
    flex: 1 1 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.gds-tasks-col-exec {
    width: 130px;
    flex-shrink: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--gds-muted-foreground);
}
.gds-tasks-col-age {
    width: 75px;
    flex-shrink: 0;
    white-space: nowrap;
    color: var(--gds-muted-foreground);
}
.gds-tasks-row--selected .gds-tasks-col-exec,
.gds-tasks-row--selected .gds-tasks-col-age,
.gds-tasks-row--selected .gds-tasks-row-id {
    color: inherit;
}
.gds-tasks-body {
    flex: 1 1 0;
    min-height: 0;
    width: 100%;
    overflow: hidden;
    display: flex;
    flex-direction: column;
}
.gds-tasks-list {
    flex: 1 1 0;
    min-height: 0;
    width: 100%;
    height: 100%;
    overflow-x: hidden;
}
.gds-tasks-row {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 30px;
    padding-right: 12px;
    font-size: var(--gds-text-xs, 12px);
    cursor: pointer;
    box-sizing: border-box;
}
.gds-tasks-row--scope {
    background-color: var(--zui-color-muted, var(--gds-surface));
}
.gds-tasks-row:hover {
    background-color: var(--gds-hover);
}
.gds-tasks-row--selected,
.gds-tasks-row--selected:hover {
    background-color: var(--gds-selection);
    color: var(--gds-selection-foreground);
}
.gds-tasks-row-name {
    flex: 1 1 0;
    min-width: 0;
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 4px;
}
.gds-tasks-row-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.gds-tasks-row-id {
    color: var(--gds-muted-foreground);
    font-family: var(--gds-font-mono);
    font-size: var(--gds-text-2xs, 10px);
    flex-shrink: 0;
}
.gds-tasks-empty {
    padding: 12px;
    flex-shrink: 0;
    font-size: var(--gds-text-sm, 14px);
    color: var(--gds-muted-foreground);
}
.gds-tasks-detail {
    max-height: 180px;
    flex-shrink: 0;
    border-top: 1px solid var(--gds-border);
    background: transparent;
    padding: 12px;
    overflow-y: auto;
    font-family: var(--gds-font-mono);
    font-size: var(--gds-text-xs, 12px);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
}
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Scope(usize, usize),
    Task(usize, usize),
}

fn build_rows(snapshot: &TaskSnapshot, collapsed: &HashSet<u64>) -> Vec<Row> {
    fn visit(
        snapshot: &TaskSnapshot,
        collapsed: &HashSet<u64>,
        parent: Option<u64>,
        depth: usize,
        out: &mut Vec<Row>,
    ) {
        for (index, scope) in snapshot.scopes.iter().enumerate() {
            if scope.parent != parent {
                continue;
            }
            out.push(Row::Scope(index, depth));
            if collapsed.contains(&scope.id) {
                continue;
            }
            for (task_index, task) in snapshot.tasks.iter().enumerate() {
                if task.scope == scope.id {
                    out.push(Row::Task(task_index, depth + 1));
                }
            }
            visit(snapshot, collapsed, Some(scope.id), depth + 1, out);
        }
    }

    let mut rows = Vec::new();
    visit(snapshot, collapsed, None, 0, &mut rows);
    rows
}

fn execution_label(execution: Execution) -> String {
    match execution {
        Execution::Supervisor => i18n::text("logs-tasks-supervisor"),
        Execution::GpuiBackground => i18n::text("logs-tasks-gpui-background"),
        Execution::Tokio => i18n::text("logs-tasks-tokio"),
        Execution::Blocking => i18n::text("logs-tasks-blocking"),
        Execution::Thread => i18n::text("logs-tasks-thread"),
    }
}

#[must_use]
pub(crate) fn task_name(task: &TaskRecord) -> String {
    if task.name.is_empty() {
        Path::new(task.source.file())
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        task.name.clone()
    }
}

#[must_use]
pub(crate) fn toggle_selected_id(current: Option<u64>, clicked: u64) -> Option<u64> {
    (current != Some(clicked)).then_some(clicked)
}

#[cfg(test)]
pub(crate) mod test_probe {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    pub(crate) struct TaskProbe {
        pub snapshot: RwSignal<TaskSnapshot, LocalStorage>,
        pub collapsed: RwSignal<HashSet<u64>, LocalStorage>,
        pub selected: RwSignal<Option<u64>, LocalStorage>,
        pub row_count: Signal<usize, LocalStorage>,
        pub tasks_empty: Signal<bool, LocalStorage>,
        pub active: Rc<Cell<bool>>,
    }

    thread_local! {
        pub(crate) static PROBE: RefCell<Option<TaskProbe>> = const { RefCell::new(None) };
    }
}

/// Live task and scope hierarchy inspector bound to the application task supervisor.
#[component]
pub fn TaskManagerView() -> impl IntoView {
    crate::ui::styles::install_ui_stylesheets();
    install_stylesheet(SHEET, TASK_MANAGER_CSS);

    let inspector = Gds::try_current().and_then(|gds| gds.tasks().inspector());
    let initial = inspector
        .as_ref()
        .map(crate::service::task_manager::Inspector::snapshot)
        .unwrap_or_default();

    let snapshot = RwSignal::new_local(initial);
    let collapsed = RwSignal::new_local(HashSet::<u64>::new());
    let selected = RwSignal::new_local(None::<u64>);

    let rows: Memo<Vec<Row>> =
        Memo::new(move |_| snapshot.with(|snap| collapsed.with(|hidden| build_rows(snap, hidden))));
    let row_count: Signal<usize, LocalStorage> = Signal::derive_local(move || rows.with(Vec::len));
    let tasks_empty: Signal<bool, LocalStorage> =
        Signal::derive_local(move || snapshot.with(|snap| snap.tasks.is_empty()));

    let refresh_handle: Option<IntervalHandle> = inspector.as_ref().map(|insp| {
        let inspector = insp.clone();
        set_interval(REFRESH, move || {
            let next = inspector.snapshot();
            collapsed.update(|hidden| {
                hidden.retain(|id| next.scopes.iter().any(|scope| scope.id == *id));
            });
            if let Some(current_selected) = selected.get_untracked()
                && !next.tasks.iter().any(|task| task.id == current_selected)
            {
                selected.set(None);
            }
            snapshot.set(next);
        })
    });
    let handle_store = StoredValue::new_local(refresh_handle);
    #[cfg(test)]
    let probe_active = std::rc::Rc::new(std::cell::Cell::new(true));
    #[cfg(test)]
    {
        let active_flag = std::rc::Rc::clone(&probe_active);
        test_probe::PROBE.with(|slot| {
            *slot.borrow_mut() = Some(test_probe::TaskProbe {
                snapshot,
                collapsed,
                selected,
                row_count,
                tasks_empty,
                active: active_flag,
            });
        });
    }
    on_cleanup_local(move || {
        handle_store.update_value(|slot| {
            *slot = None;
        });
        #[cfg(test)]
        probe_active.set(false);
    });

    let list_view = move || {
        if row_count.get() == 0 {
            return AnyView::new(());
        }

        AnyView::new(view! {
            VirtualList(
                count = row_count,
                row_size = ROW_HEIGHT,
                overscan = 4,
                label = "Tasks",
                class = "gds-tasks-list",
                row = move |index| {
                    let row_content = move || {
                        let row = rows.with(|items| items.get(index).copied())?;
                        match row {
                            Row::Scope(scope_idx, depth) => {
                                let (id, name, cancelling) = snapshot.with(|snap| {
                                    let scope = snap.scopes.get(scope_idx)?;
                                    Some((scope.id, scope.name.clone(), scope.cancelling))
                                })?;
                                let is_collapsed = collapsed.with(|hidden| hidden.contains(&id));
                                let indent_style = format!("padding-left: {}px;", 12.0 + depth as f32 * 16.0);
                                let chevron = if is_collapsed {
                                    IconName::ChevronRight
                                } else {
                                    IconName::ChevronDown
                                };
                                let badge_variant = if cancelling {
                                    BadgeVariant::Destructive
                                } else {
                                    BadgeVariant::Secondary
                                };
                                let badge_label = if cancelling {
                                    i18n::text("logs-tasks-scope-closing")
                                } else {
                                    i18n::text("logs-tasks-scope-open")
                                };
                                Some(AnyView::new(view! {
                                    box(
                                        class = "gds-tasks-row gds-tasks-row--scope",
                                        style = indent_style,
                                        attr:data-task-scope = {id.to_string()},
                                        on:click = move |_| {
                                            collapsed.update(|hidden| {
                                                if !hidden.remove(&id) {
                                                    hidden.insert(id);
                                                }
                                            });
                                        },
                                    ) {
                                        box(class = "gds-tasks-row-name") {
                                            Icon(name = chevron, size = 12.0)
                                            text(class = "gds-tasks-row-title") {{name}}
                                            text(class = "gds-tasks-row-id") {{format!("#{id}")}}
                                            Badge(variant = badge_variant) {{badge_label}}
                                        }
                                    }
                                }))
                            }
                            Row::Task(task_idx, depth) => {
                                let (id, name, cancelling, exec, age_secs) = snapshot.with(|snap| {
                                    let task = snap.tasks.get(task_idx)?;
                                    Some((
                                        task.id,
                                        task_name(task),
                                        task.cancelling,
                                        task.execution,
                                        task.age.as_secs_f64(),
                                    ))
                                })?;
                                let is_selected = selected.get() == Some(id);
                                let indent_style = format!("padding-left: {}px;", 12.0 + depth as f32 * 16.0);
                                let badge_variant = if cancelling {
                                    BadgeVariant::Destructive
                                } else {
                                    BadgeVariant::Default
                                };
                                let status_label = if cancelling {
                                    i18n::text("logs-tasks-cancelling")
                                } else {
                                    i18n::text("logs-tasks-active")
                                };
                                let exec_text = execution_label(exec);
                                let aria = format!("{name}, {exec_text}, {status_label}");
                                Some(AnyView::new(view! {
                                    box(
                                        class = "gds-tasks-row",
                                        class = if is_selected {
                                            "gds-tasks-row--selected"
                                        } else {
                                            ""
                                        },
                                        style = indent_style,
                                        a11y:label = aria,
                                        attr:data-task-entry = {id.to_string()},
                                        on:click = move |_| {
                                            selected.update(|current| {
                                                *current = toggle_selected_id(*current, id);
                                            });
                                        },
                                    ) {
                                        box(class = "gds-tasks-row-name") {
                                            text(class = "gds-tasks-row-title") {{name}}
                                            text(class = "gds-tasks-row-id") {{format!("#{id}")}}
                                            Badge(variant = badge_variant) {{status_label}}
                                        }
                                        text(class = "gds-tasks-col-exec") {{exec_text}}
                                        text(class = "gds-tasks-col-age") {{format!("{age_secs:.1} s")}}
                                    }
                                }))
                            }
                        }
                    };
                    view! { {row_content} }
                },
            )
        })
    };

    let empty_view = move || {
        tasks_empty.get().then(|| {
            view! {
                box(class = "gds-tasks-empty", attr:data-testid = "logs-tasks-empty") {
                    text {{move || i18n::text("logs-tasks-empty")}}
                }
            }
        })
    };

    let detail_view = move || {
        let sel = selected.get()?;
        let detail_text = snapshot.with(|snap| {
            snap.tasks.iter().find(|task| task.id == sel).map(|task| {
                wrap_tokens(&format!(
                    "{} · #{}\n{}",
                    task_name(task),
                    task.id,
                    task.source
                ))
            })
        })?;
        Some(view! {
            box(class = "gds-tasks-detail", attr:data-testid = "tasks-detail") {
                text {{detail_text}}
            }
        })
    };

    view! {
        box(class = "gds-tasks-root", attr:data-testid = "tasks-panel") {
            box(class = "gds-tasks-header") {
                text(class = "gds-tasks-col-name") {{move || i18n::text("logs-tasks-name")}}
                text(class = "gds-tasks-col-exec") {{move || i18n::text("logs-tasks-execution")}}
                text(class = "gds-tasks-col-age") {{move || i18n::text("logs-tasks-age")}}
            }
            box(class = "gds-tasks-body") {
                {list_view}
                {empty_view}
            }
            {detail_view}
        }
    }
}
