use std::time::Duration;

use zgui::prelude::*;

use super::{deliver, pump_frames, run_headless_logs, with_viewer_probe};
use crate::service::logs::{Level, Store};
use crate::service::task_manager::Execution;
use crate::ui::task_manager::{REFRESH, test_probe as task_probe};

fn with_task_probe<R>(f: impl FnOnce(&task_probe::TaskProbe) -> R) -> R {
    task_probe::PROBE.with(|slot| {
        let borrowed = slot.borrow();
        let probe = borrowed.as_ref().expect("task manager probe mounted");
        f(probe)
    })
}

fn tick_tasks(harness: &mut zgui_platform_headless::Harness<Box<dyn zgui::platform::AppHandler>>) {
    harness.pump();
    harness.advance(REFRESH + Duration::from_millis(5));
    pump_frames(harness);
}

#[test]
fn tasks_show_cancellation_until_cleanup_and_preserve_log_state() {
    let store = Store::new();
    run_headless_logs(true, Some(store.clone()), |harness, gds| {
        let root = gds.tasks().scope();
        let owner = root.named_child("Terminal");
        let completion = owner
            .register_named("PTY cleanup", Execution::GpuiBackground)
            .expect("register test background task");
        let snapshot = root.snapshot();
        let task_id = snapshot.tasks[0].id;
        let scope_id = snapshot.scopes[1].id;

        let log_id = store.record(Level::Info, "test", "matching entry");
        deliver(harness);
        with_viewer_probe(|probe| {
            probe.query.set("matching".to_owned());
            probe.selected.set(Some(log_id));
            probe.active_tab.set("tasks".to_owned());
        });
        pump_frames(harness);

        with_task_probe(|probe| {
            assert!(probe.active.get());
            let snap = probe.snapshot.get_untracked();
            assert!(snap.scopes.iter().any(|s| s.id == scope_id));
            assert!(snap.tasks.iter().any(|t| t.id == task_id && !t.cancelling));
            assert_eq!(probe.row_count.get_untracked(), 3);

            probe.collapsed.update(|hidden| {
                hidden.insert(scope_id);
            });
        });
        pump_frames(harness);

        with_task_probe(|probe| {
            assert_eq!(probe.row_count.get_untracked(), 2);
            probe.collapsed.update(|hidden| {
                hidden.remove(&scope_id);
            });
            probe.selected.set(Some(task_id));
        });
        pump_frames(harness);

        drop(owner);
        store.record(Level::Warn, "test", "matching second entry");
        tick_tasks(harness);
        with_task_probe(|probe| {
            let snap = probe.snapshot.get_untracked();
            assert!(snap.scopes.iter().any(|s| s.id == scope_id && s.cancelling));
            assert!(snap.tasks.iter().any(|t| t.id == task_id && t.cancelling));
        });

        drop(completion);
        tick_tasks(harness);
        with_task_probe(|probe| {
            assert_eq!(probe.row_count.get_untracked(), 1);
            assert!(
                probe.tasks_empty.get_untracked(),
                "logs-tasks-empty must show whenever snapshot.tasks is empty, even with active scopes"
            );
            assert!(
                !probe
                    .snapshot
                    .with_untracked(|s| s.tasks.iter().any(|t| t.id == task_id))
            );
            assert_eq!(probe.selected.get_untracked(), None);
        });

        with_viewer_probe(|probe| {
            probe.active_tab.set("logs".to_owned());
        });
        pump_frames(harness);

        with_task_probe(|probe| {
            assert!(
                !probe.active.get(),
                "task refresh interval must stop when switching back to logs tab"
            );
        });
        deliver(harness);
        with_viewer_probe(|probe| {
            assert_eq!(probe.query.get_untracked(), "matching");
            assert_eq!(probe.selected.get_untracked(), Some(log_id));
            assert_eq!(probe.visible.get_untracked().len(), 2);
        });
    });
}

#[test]
fn hiding_and_closing_inspector_never_cancels_work() {
    let store = Store::new();
    run_headless_logs(true, Some(store), |harness, gds| {
        let root = gds.tasks().scope();
        let owner = root.named_child("Preview");
        let completion = owner
            .register_named("Decode", Execution::Blocking)
            .expect("register blocking task");

        with_viewer_probe(|probe| {
            probe.active_tab.set("tasks".to_owned());
        });
        pump_frames(harness);

        with_task_probe(|probe| {
            assert!(probe.active.get());
        });

        with_viewer_probe(|probe| {
            probe.active_tab.set("logs".to_owned());
        });
        pump_frames(harness);

        with_task_probe(|probe| {
            assert!(!probe.active.get());
        });

        assert!(!owner.token().is_cancelled());
        assert_eq!(root.snapshot().tasks.len(), 1);
        drop(completion);
        assert!(root.snapshot().tasks.is_empty());
    });
}

#[test]
fn unnamed_task_uses_source_file_basename_and_reclick_toggles_selection_off() {
    use crate::ui::task_manager::{task_name, toggle_selected_id};

    assert_eq!(toggle_selected_id(None, 7), Some(7));
    assert_eq!(toggle_selected_id(Some(7), 7), None);

    let store = Store::new();
    run_headless_logs(true, Some(store), |harness, gds| {
        let root = gds.tasks().scope();
        let unnamed = root.register().expect("register unnamed task");
        let named = root
            .register_named("Indexer", Execution::Tokio)
            .expect("register named task");

        let snap = root.snapshot();
        let unnamed_rec = snap
            .tasks
            .iter()
            .find(|t| t.name.is_empty())
            .expect("unnamed task snapshot");
        let named_rec = snap
            .tasks
            .iter()
            .find(|t| t.name == "Indexer")
            .expect("named task snapshot");
        assert_eq!(task_name(unnamed_rec), "tasks.rs");
        assert_eq!(task_name(named_rec), "Indexer");

        with_viewer_probe(|probe| {
            probe.active_tab.set("tasks".to_owned());
        });
        pump_frames(harness);

        with_task_probe(|probe| {
            assert!(!probe.tasks_empty.get_untracked());
        });

        drop(unnamed);
        drop(named);
        tick_tasks(harness);

        with_task_probe(|probe| {
            assert!(probe.row_count.get_untracked() >= 1);
            assert!(probe.tasks_empty.get_untracked());
        });
    });
}
