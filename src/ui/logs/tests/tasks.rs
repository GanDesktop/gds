use super::*;
use super::{deliver, fixture};
use crate::service::task_manager::Execution;
use crate::ui::task_manager::REFRESH;
use gpui_kit::{TestAppContext, test::TestWindowExt};

fn tick(cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.executor().advance_clock(REFRESH);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn tasks_show_cancellation_until_cleanup_and_preserve_log_state(cx: &mut TestAppContext) {
    let (handle, viewer, store) = fixture(cx);
    let root = cx.update(|cx| cx.gds().tasks().scope());
    let owner = root.named_child("Terminal");
    let completion = owner
        .register_named("PTY cleanup", Execution::GpuiBackground)
        .unwrap();
    let snapshot = root.snapshot();
    let task = snapshot.tasks[0].id;
    let scope = snapshot.scopes[1].id;
    let log = store.record(Level::Info, "test", "matching entry");
    deliver(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.input("matching", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("log-entry", log), cx);
        window.render_frame(cx);
        assert_eq!(window.find(1usize).label(), Some("任务"));
        window.click(1usize, cx);
        window.render_frame(cx);
        assert!(window.find("tasks-panel").visible());
        assert!(window.try_find("logs-toolbar").is_none());
        assert!(
            window
                .find(("task-entry", task))
                .label()
                .unwrap()
                .contains("活动")
        );
        window.click(("task-scope", scope), cx);
        window.render_frame(cx);
        assert!(window.try_find(("task-entry", task)).is_none());
        window.click(("task-scope", scope), cx);
        window.render_frame(cx);
        window.click(("task-entry", task), cx);
        window.render_frame(cx);
        assert!(window.find("tasks-detail").visible());
        viewer
            .update(cx, |view, cx| {
                assert!(
                    view.tasks
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .focus
                        .is_focused(window)
                )
            })
            .unwrap();
    })
    .unwrap();
    drop(owner);
    store.record(Level::Warn, "test", "matching second entry");
    tick(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find(("task-entry", task))
                .label()
                .unwrap()
                .contains("取消中")
        );
        assert!(window.find(("task-scope", scope)).visible());
    })
    .unwrap();
    drop(completion);
    tick(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("task-entry", task)).is_none());
        assert!(window.try_find(("task-scope", scope)).is_none());
        assert!(window.try_find("tasks-detail").is_none());
        assert!(window.find("logs-tasks-empty").visible());
        window.click(0usize, cx);
        window.render_frame(cx);
        assert!(window.find("logs-detail").visible());
        viewer
            .update(cx, |view, cx| {
                assert_eq!(view.query, "matching");
                assert_eq!(view.selected, Some(log));
                assert_eq!(view.visible.len(), 2);
                assert!(!view.tasks.as_ref().unwrap().read(cx).is_active());
            })
            .unwrap();
    })
    .unwrap();
}

#[gpui_kit::test]
fn hiding_and_closing_inspector_never_cancels_work(cx: &mut TestAppContext) {
    let (handle, viewer, _) = fixture(cx);
    let root = cx.update(|cx| cx.gds().tasks().scope());
    let owner = root.named_child("Preview");
    let completion = owner.register_named("Decode", Execution::Blocking).unwrap();
    let tasks = cx.update(|cx| {
        viewer
            .upgrade()
            .unwrap()
            .read(cx)
            .tasks
            .as_ref()
            .unwrap()
            .downgrade()
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(1usize, cx);
        window.render_frame(cx);
        window.click(0usize, cx);
        window.render_frame(cx);
        viewer
            .update(cx, |view, cx| {
                assert!(!view.tasks.as_ref().unwrap().read(cx).is_active())
            })
            .unwrap();
        window.click(1usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    let again = cx.update(|cx| open(cx).unwrap());
    assert_eq!(handle.window_id(), again.window_id());
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        viewer
            .update(cx, |view, cx| {
                assert!(
                    view.tasks
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .focus
                        .is_focused(window)
                )
            })
            .unwrap();
        window.press(
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-w"
            },
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    assert!(viewer.upgrade().is_none());
    assert!(tasks.upgrade().is_none());
    assert!(!owner.token().is_cancelled());
    assert_eq!(root.snapshot().tasks.len(), 1);
    drop(completion);
    assert!(root.snapshot().tasks.is_empty());
}

#[gpui_kit::test]
fn long_task_names_keep_columns_and_details_inside_small_window(cx: &mut TestAppContext) {
    let (handle, _, _) = fixture(cx);
    let root = cx.update(|cx| cx.gds().tasks().scope());
    let owner = root.named_child("Preview / ".repeat(100));
    let _completion = owner
        .register_named("目录/".repeat(200), Execution::Thread)
        .unwrap();
    let task = root.snapshot().tasks[0].id;
    for font_size in [15., 24.] {
        cx.update(|cx| gpui_kit::component::Theme::global_mut(cx).font_size = px(font_size));
        cx.update_window(handle.into(), |_, window, cx| {
            window.resize(size(
                px(super::super::window::MIN_WIDTH),
                px(super::super::window::MIN_HEIGHT),
            ));
            window.bounds_changed(cx);
            window.render_frame(cx);
            window.click(1usize, cx);
            window.render_frame(cx);
            if window.try_find("tasks-detail").is_none() {
                window.click(("task-entry", task), cx);
                window.render_frame(cx);
            }
            let panel = window.find("tasks-panel").bounds();
            let row = window.find(("task-entry", task)).bounds();
            let detail = window.find("tasks-detail").bounds();
            assert!(row.size.width > px(0.));
            assert!(row.left() >= panel.left() && row.right() <= panel.right());
            assert!(detail.size.height <= px(DETAIL_HEIGHT));
            assert!(detail.right() <= panel.right() && detail.bottom() <= panel.bottom());
        })
        .unwrap();
    }
}
