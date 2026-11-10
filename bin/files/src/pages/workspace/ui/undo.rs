//! File-operation history belongs to the window, across folders and file tabs.

use super::*;
use crate::features::file_operations::history::{Direction, Report};
use fluent_bundle::FluentArgs;

impl Browser {
    pub(super) fn history_available(&self, command: Command, cx: &App) -> bool {
        !self.operations.in_progress
            && !self.collections.trash.is_busy()
            && !self.tabs.items.iter().any(|tab| {
                tab.remote.as_ref().is_some_and(|remote| {
                    remote
                        .view
                        .as_ref()
                        .is_some_and(|view| view.read(cx).busy())
                })
            })
            && self
                .operations
                .history
                .peek(direction(command))
                .is_some_and(|entry| {
                    entry
                        .paths()
                        .iter()
                        .all(|path| self.device_path_accepts_task(path, cx))
                })
    }

    pub(super) fn history_label(&self, command: Command, cx: &App) -> SharedString {
        let Some(entry) = self.operations.history.peek(direction(command)) else {
            return i18n::text(command.label_id(), cx);
        };
        let mut args = FluentArgs::new();
        args.set(
            "operation",
            i18n::text(entry.kind.label_id(), cx).to_string(),
        );
        i18n::format(
            if command == Command::Undo {
                "undo-operation"
            } else {
                "redo-operation"
            },
            Some(&args),
            cx,
        )
    }

    pub(super) fn replay_file_operation(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.history_available(command, cx) {
            return;
        }
        let direction = direction(command);
        let paths = self
            .operations
            .history
            .peek(direction)
            .expect("available history")
            .paths();
        if paths
            .iter()
            .any(|path| !self.device_path_accepts_task(path, cx))
        {
            notifications::error(i18n::text("device-path-unavailable", cx), cx);
            return;
        }
        let label = self.history_label(command, cx);
        let entry = self
            .operations
            .history
            .take(direction)
            .expect("available history");
        self.operations.in_progress = true;
        self.operations.paths = paths;
        self.window.focus.focus(window, cx);
        self.start_background(label, cx);
        let cancel = self.background_tasks.read(cx).token("files");
        let work = gds::ui::task_manager::scope(cx)
            .spawn_blocking(&self.window.runtime, move || {
                entry.execute_cancellable(&cancel)
            })
            .expect("undo accepted");
        cx.spawn(async move |this, cx| {
            let result = work.await;
            if let Err(error) = this.update(cx, |this, cx| {
                this.operations.in_progress = false;
                this.operations.paths.clear();
                match result {
                    Ok(report) => this.finish_history(direction, report, cx),
                    Err(error) => {
                        this.operations.history.clear();
                        this.finish_background(
                            notifications::Report::new(
                                notifications::Level::Error,
                                error.to_string(),
                            ),
                            cx,
                        );
                    }
                }
                cx.notify();
            }) {
                tracing::warn!("File history result arrived after the window closed: {error}");
            }
        })
        .detach();
        cx.notify();
    }

    fn finish_history(&mut self, direction: Direction, mut report: Report, cx: &mut Context<Self>) {
        self.operations.history.finish(direction, &mut report);
        self.clear_completed_selection(&report.changed);
        self.refresh_after_file_operation(&report.changed, cx);
        self.load_trash(cx);
        let failed = !report.errors.is_empty();
        let mut notice = notifications::Report::new(
            if failed {
                notifications::Level::Error
            } else {
                notifications::Level::Success
            },
            i18n::text(
                match (direction, failed) {
                    (Direction::Undo, false) => "undo-done",
                    (Direction::Redo, false) => "redo-done",
                    (Direction::Undo, true) => "undo-failed",
                    (Direction::Redo, true) => "redo-failed",
                },
                cx,
            ),
        );
        for error in report.errors {
            notice = notice.detail(error);
        }
        self.finish_background(notice, cx);
    }
}

fn direction(command: Command) -> Direction {
    if command == Command::Undo {
        Direction::Undo
    } else {
        Direction::Redo
    }
}
