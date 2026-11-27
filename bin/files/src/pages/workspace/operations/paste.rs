//! Local paste, external file-drop transfer, and conflict-resolution orchestration.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use gds::i18n as gds_i18n;
use gds::ui::drag::ExternalFileDrop;
use tokio_util::sync::CancellationToken;
use zgui::prelude::{DragAction, DropHandle};
use zgui::reactive::spawn_local;

use super::super::{navigation::Navigator, state::context::Context};
use super::lifecycle::Lifecycle;
use crate::{
    features::file_operations::{
        history,
        paste::{self, Inspection, PassOutcome, PasteJob},
        transfer,
    },
    shared::{
        i18n,
        lib::clipboard::{self, Clipboard},
    },
};

#[derive(Clone)]
struct PasteContext {
    cut: bool,
    record_history: bool,
    snapshot: Option<clipboard::Snapshot>,
    drop_handle: Rc<RefCell<Option<DropHandle>>>,
    inhibit: Rc<RefCell<Option<gds::service::desktop::InhibitGuard>>>,
}

impl PasteContext {
    fn new(
        cut: bool,
        record_history: bool,
        snapshot: Option<clipboard::Snapshot>,
        drop_handle: Option<DropHandle>,
    ) -> Self {
        Self {
            cut,
            record_history,
            snapshot,
            drop_handle: Rc::new(RefCell::new(drop_handle)),
            inhibit: Rc::new(RefCell::new(None)),
        }
    }

    fn begin_transfer_inhibit(&self, scope: &gds::service::task_manager::Scope) {
        let mut slot = self.inhibit.borrow_mut();
        if slot.is_some() {
            return;
        }
        *slot = Some(gds::service::desktop::InhibitGuard::delayed(
            scope,
            gds::service::desktop::DEFAULT_INHIBIT_DELAY,
            i18n::text_untracked("transfer-inhibit-reason"),
        ));
    }

    fn settle_drop(&self, succeeded: bool) {
        let Some(handle) = self.drop_handle.borrow_mut().take() else {
            return;
        };
        if succeeded {
            handle.complete();
        } else {
            handle.reject();
        }
    }

    fn settle(&self, succeeded: bool) {
        self.settle_drop(succeeded);
        let _ = self.inhibit.borrow_mut().take();
    }
}

pub(in crate::pages::workspace) struct PasteOps<'a>(pub &'a Context);

impl PasteOps<'_> {
    pub(super) fn start_local_paste(&self, destination: PathBuf, snapshot: clipboard::Snapshot) {
        let cut = snapshot.mode == clipboard::Mode::Cut;
        let mode = if cut {
            transfer::Mode::Move
        } else {
            transfer::Mode::Copy
        };
        let job = PasteJob::new(destination, mode, snapshot.paths.clone());
        self.start_paste_job(job, PasteContext::new(cut, true, Some(snapshot), None));
    }

    pub(in crate::pages::workspace) fn handle_external_file_drop(
        &self,
        destination: PathBuf,
        drop: ExternalFileDrop,
    ) {
        let ExternalFileDrop {
            paths,
            action,
            handle,
            ..
        } = drop;
        if paths.is_empty() {
            if let Some(handle) = handle {
                handle.reject();
            }
            self.0
                .report_notice(i18n::text_untracked("drop-no-local-files"));
            return;
        }
        let mode = if action == Some(DragAction::Move) {
            transfer::Mode::Move
        } else {
            transfer::Mode::Copy
        };
        let invalid_self_target = paths
            .iter()
            .any(|src| destination == *src || destination.starts_with(src));
        if invalid_self_target {
            if let Some(handle) = handle {
                handle.reject();
            }
            self.0.report_notice(i18n::text_untracked("drop-into-self"));
            return;
        }
        let same_dir = mode == transfer::Mode::Move
            && paths
                .iter()
                .all(|path| path.parent() == Some(destination.as_path()));
        if same_dir {
            if let Some(handle) = handle {
                handle.reject();
            }
            return;
        }
        let job = PasteJob::new(destination, mode, paths);
        self.start_paste_job(job, PasteContext::new(false, true, None, handle));
    }

    pub(in crate::pages::workspace) fn inspect_paste(
        &self,
        transfer: paste::PasteJob,
        cut: bool,
        history: bool,
        finish: Option<Vec<String>>,
    ) {
        let snapshot = if cut && finish.is_some() {
            Clipboard::current()
                .snapshot_untracked()
                .map(|snap| (*snap).clone())
        } else {
            None
        };
        self.start_paste_job(transfer, PasteContext::new(cut, history, snapshot, None));
    }

    fn start_paste_job(&self, job: PasteJob, context: PasteContext) {
        let Some(cancel) =
            Lifecycle(self.0).begin_operation(&i18n::text_untracked("paste-working"))
        else {
            context.settle(false);
            self.0
                .report_notice(i18n::text_untracked("file-operation-busy"));
            return;
        };
        context.begin_transfer_inhibit(&self.0.scope());
        self.inspect_paste_step(job, context, cancel);
    }

    fn inspect_paste_step(&self, job: PasteJob, context: PasteContext, cancel: CancellationToken) {
        let task = self
            .0
            .scope()
            .spawn_blocking_named("Inspect paste conflicts", move || job.inspect());
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(ctx) = weak.upgrade() else {
                    context.settle(false);
                    return;
                };
                let Ok(inspection) = result else {
                    Lifecycle(&ctx).end_operation();
                    context.settle(false);
                    if let Err(error) = result {
                        ctx.report_notice(error.to_string());
                    }
                    return;
                };
                let paste_ops = PasteOps(&ctx);
                match inspection {
                    Inspection::Ready(ready_job) => {
                        paste_ops.execute_paste_step(ready_job, context, cancel);
                    }
                    Inspection::Done(done_job) => {
                        paste_ops.finish_paste_job(done_job, context, false);
                    }
                    Inspection::Failed { job, error } => {
                        if !job.has_disk_progress() {
                            Lifecycle(&ctx).end_operation();
                            context.settle(false);
                            ctx.report_notice(error.to_string());
                        } else {
                            let mut failed_job = job;
                            failed_job.record_abort(error.to_string());
                            paste_ops.finish_paste_job(failed_job, context, true);
                        }
                    }
                    Inspection::Conflict { job, conflict } => {
                        paste_ops.ask_paste_conflict(job, conflict, context, cancel);
                    }
                }
            }));
        });
    }

    fn ask_paste_conflict(
        &self,
        job: PasteJob,
        conflict: paste::PasteConflict,
        context: PasteContext,
        cancel: CancellationToken,
    ) {
        let mode = job.mode;
        let conflict_for_resolve = conflict.clone();
        let ctx_resolve = self.0.clone();
        let ctx_cancel = self.0.clone();
        let shared_job = Rc::new(RefCell::new(Some(job)));
        let resolve_job = Rc::clone(&shared_job);
        let cancel_job = shared_job;
        let resolve_context = context.clone();
        let cancel_context = context;
        self.0.conflict_prompt(
            mode,
            conflict,
            move |resolution| {
                let Some(mut next_job) = resolve_job.borrow_mut().take() else {
                    return;
                };
                next_job.resolve_conflict(conflict_for_resolve.clone(), resolution);
                PasteOps(&ctx_resolve).inspect_paste_step(
                    next_job,
                    resolve_context.clone(),
                    cancel.clone(),
                );
            },
            move || {
                let Some(mut cancelled_job) = cancel_job.borrow_mut().take() else {
                    return;
                };
                // Cancelling a conflict prompt means the batch did not finish cleanly; reject any
                // pending Wayland drop offer immediately so a move source never deletes files.
                cancel_context.settle_drop(false);
                cancelled_job.abort_remaining();
                if !cancelled_job.has_recorded_outcomes() {
                    Lifecycle(&ctx_cancel).end_operation();
                    cancel_context.settle(false);
                    ctx_cancel.report_notice(i18n::text_untracked("paste-cancelled"));
                } else {
                    PasteOps(&ctx_cancel).finish_paste_job(
                        cancelled_job,
                        cancel_context.clone(),
                        true,
                    );
                }
            },
        );
    }

    fn execute_paste_step(
        &self,
        mut job: PasteJob,
        context: PasteContext,
        cancel: CancellationToken,
    ) {
        let buffer_bytes = self
            .0
            .0
            .preferences
            .snapshot()
            .files
            .transfer
            .local_buffer_bytes();
        let batch = match job
            .prepare_batch()
            .map(|batch| batch.with_buffer_size(buffer_bytes))
        {
            Ok(batch) => batch,
            Err(error) => {
                if !job.has_disk_progress() {
                    Lifecycle(self.0).end_operation();
                    context.settle(false);
                    self.0.report_notice(error.to_string());
                } else {
                    job.record_abort(error.to_string());
                    self.finish_paste_job(job, context, true);
                }
                return;
            }
        };

        let task_cancel = cancel.clone();
        let task = self
            .0
            .scope()
            .spawn_blocking_named("Transfer pasted files", move || {
                let report = batch.execute_cancellable(task_cancel);
                match job.apply_pass(report) {
                    PassOutcome::Completed(mut done) => {
                        done.cleanup_merged_sources();
                        PassOutcome::Completed(done)
                    }
                    PassOutcome::RetryConflict(next_job) => PassOutcome::RetryConflict(next_job),
                }
            });
        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task.await;
                let Some(ctx) = weak.upgrade() else {
                    context.settle(false);
                    return;
                };
                let paste_ops = PasteOps(&ctx);
                match result {
                    Ok(PassOutcome::Completed(done)) => {
                        paste_ops.finish_paste_job(done, context, false);
                    }
                    Ok(PassOutcome::RetryConflict(next_job)) => {
                        paste_ops.inspect_paste_step(next_job, context, cancel);
                    }
                    Err(error) => {
                        Lifecycle(&ctx).end_operation();
                        context.settle(false);
                        ctx.report_notice(error.to_string());
                    }
                }
            }));
        });
    }

    fn finish_paste_job(&self, job: PasteJob, context: PasteContext, aborted: bool) {
        let destination = job.destination.clone();
        if context.cut
            && let Some(moved) = job.completed_cut_paths()
            && let Some(snap) = context.snapshot.clone()
        {
            let weak = self.0.downgrade();
            self.0.0.owner.with(|| {
                drop(spawn_local(async move {
                    let clipboard_error = Clipboard::current()
                        .finish_cut(snap, moved)
                        .await
                        .err()
                        .map(|error| error.to_string());
                    let Some(ctx) = weak.upgrade() else {
                        context.settle(false);
                        return;
                    };
                    PasteOps(&ctx).complete_paste_summary(
                        job.into_summary(clipboard_error),
                        destination,
                        context,
                        aborted,
                    );
                }));
            });
            return;
        }
        self.complete_paste_summary(job.into_summary(None), destination, context, aborted);
    }

    fn complete_paste_summary(
        &self,
        summary: paste::Summary,
        destination: PathBuf,
        context: PasteContext,
        aborted: bool,
    ) {
        let failed_count = summary.failures.len();
        let whole_batch_succeeded = failed_count == 0
            && summary.clipboard_error.is_none()
            && !aborted
            && summary.skipped == 0
            && (!summary.succeeded.is_empty() || summary.merged_dirs > 0);
        context.settle(whole_batch_succeeded);

        let lifecycle = Lifecycle(self.0);
        lifecycle.end_operation();
        let mut changed_paths = vec![destination];
        changed_paths.extend(summary.succeeded.iter().cloned());
        changed_paths.extend(summary.committed.iter().cloned());
        changed_paths.extend(summary.touched_dirs.iter().cloned());
        changed_paths.extend(summary.undo.iter().flat_map(history::Step::paths));
        if context.record_history {
            let kind = summary.history_kind();
            let reset = summary.requires_history_reset();
            if reset {
                lifecycle.clear_history();
            }
            if !summary.non_undoable {
                lifecycle.record_history(history::Entry::new(kind, summary.undo));
            }
        }
        lifecycle.refresh_affected(&changed_paths);
        if !summary.committed.is_empty() {
            Navigator(self.0).reveal_paths(summary.committed);
        }

        if failed_count > 0 || summary.clipboard_error.is_some() || aborted {
            let headline = if aborted && failed_count == 0 {
                gds::t_untracked!(
                    "paste-aborted-partial",
                    completed = summary.succeeded.len() as i64
                )
            } else {
                gds::t_untracked!(
                    "transfer-partial",
                    succeeded = summary.succeeded.len() as i64,
                    failed = failed_count as i64
                )
            };
            let mut details = summary.failures;
            if let Some(err) = summary.clipboard_error {
                details.push(gds::t_untracked!("clipboard-update-failed", error = err));
            }
            let message = if details.is_empty() {
                headline.clone()
            } else {
                format!("{headline}\n{}", details.join("\n"))
            };
            self.0.report_notice(message);
            self.0.notify_if_unfocused(
                "local-transfer",
                i18n::text_untracked("transfer-notify-failed-title"),
                headline,
                true,
            );
        } else if !summary.succeeded.is_empty() && summary.skipped > 0 {
            let message = if summary.mode == transfer::Mode::Move {
                gds::t_untracked!(
                    "move-done-skipped",
                    succeeded = summary.succeeded.len() as i64,
                    skipped = summary.skipped as i64
                )
            } else {
                gds::t_untracked!(
                    "copy-done-skipped",
                    succeeded = summary.succeeded.len() as i64,
                    skipped = summary.skipped as i64
                )
            };
            self.0.report_success(message.clone());
            self.0.notify_if_unfocused(
                "local-transfer",
                i18n::text_untracked("transfer-notify-title"),
                message,
                false,
            );
        } else if !summary.succeeded.is_empty() {
            let key = if summary.mode == transfer::Mode::Move {
                "move-done"
            } else {
                "copy-done"
            };
            let message = gds_i18n::count_untracked(key, summary.succeeded.len());
            self.0.report_success(message.clone());
            self.0.notify_if_unfocused(
                "local-transfer",
                i18n::text_untracked("transfer-notify-title"),
                message,
                false,
            );
        } else if summary.merged_dirs > 0 && summary.skipped == 0 {
            let message = gds_i18n::count_untracked("paste-merged", summary.merged_dirs);
            self.0.report_success(message.clone());
            self.0.notify_if_unfocused(
                "local-transfer",
                i18n::text_untracked("transfer-notify-title"),
                message,
                false,
            );
        } else if summary.skipped > 0 {
            self.0
                .report_info(gds_i18n::count_untracked("paste-skipped", summary.skipped));
        }
        self.0.focus_content();
    }
}
