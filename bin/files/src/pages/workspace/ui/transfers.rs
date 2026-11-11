//! Paste transfers with pre-flight conflict naming.
//!
//! A paste captures the clipboard snapshot and the destination when it starts,
//! so navigating elsewhere cannot retarget it. Existing or duplicated target
//! names are named interactively before any filesystem work; the transfer
//! itself still commits every entry atomically without replacing anything.
//! Probing the destination and suggesting a free name run on the background
//! executor, so a slow or network mount never blocks the UI.

mod conflict;

use gds::{i18n, ui::notifications};

use super::*;
use crate::features::file_operations::{
    history,
    paste::{Ending, Inspection, PassOutcome, PasteConflict, PasteJob, Summary},
    transfer,
};
use crate::shared::{
    i18n::t,
    lib::clipboard::{self as file_clipboard, Bridge, Mode, Snapshot},
};
use conflict::ConflictDialog;

/// UI context for one active paste flow across dialogs and transfer passes.
struct Paste {
    origin_tab: u64,
    clipboard: Bridge,
    snapshot: Snapshot,
    job: PasteJob,
}

impl Paste {
    fn with_job(self, job: PasteJob) -> Self {
        Self { job, ..self }
    }
}

impl Browser {
    pub(super) fn cancel_clipboard_cut(&self, cx: &mut Context<Self>) -> bool {
        if self.operations.in_progress {
            return false;
        }
        let Some(cancel) = file_clipboard::cancel_cut(cx) else {
            return false;
        };
        cx.spawn(async move |_, cx| {
            if let Err(error) = cancel.await {
                cx.update(|cx| {
                    let error = error.to_string();
                    notifications::error(t!(@cx, "clipboard-cancel-failed", error), cx);
                });
            }
        })
        .detach();
        true
    }

    pub(super) fn copy_files(&mut self, cut: bool, _: &mut Window, cx: &mut Context<Self>) {
        let paths = self.selected_visible_paths();
        self.copy_file_paths(paths, cut, cx);
    }

    pub(super) fn copy_file_paths(
        &mut self,
        paths: Vec<PathBuf>,
        cut: bool,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let count = paths.len();
        let mode = if cut { Mode::Cut } else { Mode::Copy };
        let clipboard = Bridge::new(cx);
        let write = clipboard.write(mode, paths, cx);
        cx.spawn(async move |this, cx| {
            let result = write.await;
            if let Err(closed) = this.update(cx, |_, cx| {
                match result {
                    Ok(snapshot) => {
                        file_clipboard::note_clipboard_write(&snapshot, cx);
                        notifications::Report::new(
                            notifications::Level::Success,
                            i18n::count(
                                if cut { "cut-prepared" } else { "copy-prepared" },
                                count,
                                cx,
                            ),
                        )
                        .replacing::<ClipboardNotice>()
                        .show(cx);
                    }
                    Err(error) => {
                        // A failed write leaves whatever the clipboard held before;
                        // never mark a cut that was not published.
                        notifications::Report::new(notifications::Level::Error, error.to_string())
                            .replacing::<ClipboardNotice>()
                            .show(cx);
                    }
                }
                cx.notify();
            }) {
                tracing::warn!("Clipboard result arrived after the view closed: {closed}");
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn paste_files(
        &mut self,
        destination: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.in_progress
            || !self.pages.is_empty()
            || self.collection().is_some()
            || !self.device_path_accepts_task(&destination, cx)
        {
            return;
        }
        let clipboard = Bridge::new(cx);
        let read = clipboard.read(cx);
        let origin_tab = self.tabs.active_id;
        self.operations.in_progress = true;
        // Until the clipboard arrives, the source volumes are unknown.
        self.operations.paths.clear();
        self.start_background(i18n::text("paste-working", cx), cx);
        cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            if matches!(
                &result,
                Ok(None) | Err(file_clipboard::Error::InvalidUri(_))
            ) {
                let Ok(read) = this.update_in(cx, |_, _, cx| clipboard.read_uris(cx)) else {
                    return;
                };
                let uris = read.await;
                let _ = this.update_in(cx, |browser, _, cx| {
                    browser.paste_remote_uris(uris, destination, cx)
                });
                return;
            }
            let snapshot = match result {
                Ok(Some(snapshot)) => snapshot,
                Ok(None) => {
                    let _ = this.update_in(cx, |browser, _, cx| {
                        browser.fail_paste(
                            i18n::text("clipboard-no-files", cx).to_string(),
                            notifications::Level::Info,
                            cx,
                        );
                    });
                    return;
                }
                Err(error) => {
                    let _ = this.update_in(cx, |browser, _, cx| {
                        browser.fail_paste(error.to_string(), notifications::Level::Error, cx);
                    });
                    return;
                }
            };
            let mode = match snapshot.mode {
                Mode::Copy => transfer::Mode::Copy,
                Mode::Cut => transfer::Mode::Move,
            };
            let job = PasteJob::new(destination, mode, snapshot.paths.clone());
            let paste = Paste {
                origin_tab,
                clipboard,
                snapshot,
                job,
            };
            let _ = this.update_in(cx, |browser, window, cx| {
                if paste
                    .job
                    .pending
                    .iter()
                    .any(|path| !browser.device_path_available(path, cx))
                    || !browser.device_path_available(&paste.job.destination, cx)
                {
                    browser.fail_paste(
                        i18n::text("device-path-unavailable", cx).to_string(),
                        notifications::Level::Error,
                        cx,
                    );
                    return;
                }
                browser.operations.paths = paste.job.involved_paths();
                browser.inspect_paste(paste, window, cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// Continue a flow with a finished background pre-flight: commit it, report
    /// a failed inspection, or ask the user to name the first conflict.
    fn present_conflicts(
        &mut self,
        paste: Paste,
        inspection: Inspection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.background_tasks.read(cx).token("files").is_cancelled() {
            let paste = paste.with_job(inspection.into_job());
            self.cancel_paste(paste, window, cx);
            return;
        }
        match inspection {
            Inspection::Ready(job) => self.execute_paste(paste.with_job(job), window, cx),
            Inspection::Failed { job, error } => {
                self.abort_paste(paste.with_job(job), error.to_string(), window, cx);
            }
            Inspection::Conflict { job, conflict } => {
                self.ask_conflict(paste.with_job(job), conflict, window, cx);
            }
        }
    }

    /// Show the conflict dialog for one conflict that already has a suggestion.
    fn ask_conflict(
        &mut self,
        paste: Paste,
        conflict: PasteConflict,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dialog = ConflictDialog::new(cx.entity().downgrade(), paste, conflict, window, cx);
        ConflictDialog::open(dialog, window);
        cx.notify();
    }

    /// Recheck every pending source off the UI thread, then present the next
    /// conflict or commit the flow.
    fn inspect_paste(&mut self, mut paste: Paste, window: &mut Window, cx: &mut Context<Self>) {
        let job = paste.job.take();
        let work = self
            .window
            .tasks
            .spawn_blocking_named("Inspect paste conflicts", move || job.inspect());
        cx.spawn_in(window, async move |this, cx| {
            let inspection = work.await;
            let _ = this.update_in(cx, |browser, window, cx| match inspection {
                Ok(inspection) => browser.present_conflicts(paste, inspection, window, cx),
                Err(error) => {
                    browser.fail_paste(error.to_string(), notifications::Level::Error, cx)
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn execute_paste(&mut self, paste: Paste, window: &mut Window, cx: &mut Context<Self>) {
        if paste
            .job
            .pending
            .iter()
            .any(|path| !self.device_path_available(path, cx))
            || !self.device_path_available(&paste.job.destination, cx)
        {
            self.abort_paste(
                paste,
                i18n::text("device-path-unavailable", cx).to_string(),
                window,
                cx,
            );
            return;
        }
        let count = paste.job.pending.len();
        let batch = match paste.job.prepare_batch() {
            Ok(batch) => batch,
            Err(error) => {
                self.abort_paste(paste, error.to_string(), window, cx);
                return;
            }
        };
        self.start_background(i18n::count("transfer-working", count, cx), cx);
        let cancel = self.background_tasks.read(cx).token("files");
        let work = self
            .window
            .tasks
            .spawn_blocking_named("Transfer pasted files", move || {
                batch.execute_cancellable(cancel)
            });
        cx.spawn_in(window, async move |this, cx| {
            let report = work.await;
            let _ = this.update_in(cx, |browser, window, cx| match report {
                Ok(report) => browser.finish_pass(paste, report, window, cx),
                Err(error) => browser.abort_paste(paste, error.to_string(), window, cx),
            });
        })
        .detach();
        cx.notify();
    }

    /// Merge one completed pass; a conflict that appeared after naming is
    /// prompted again instead of overwriting the new target.
    fn finish_pass(
        &mut self,
        mut paste: Paste,
        report: transfer::Report,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let job = paste.job.take();
        match job.apply_pass(report) {
            PassOutcome::Completed(job) => {
                self.complete_paste(paste.with_job(job), Ending::Completed, window, cx);
            }
            PassOutcome::RetryConflict(job) => {
                self.inspect_paste(paste.with_job(job), window, cx);
            }
        }
    }

    /// Finish a flow, cleaning the clipboard for cut sources that really moved.
    fn complete_paste(
        &mut self,
        paste: Paste,
        ending: Ending,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paste.job.mode == transfer::Mode::Move && !paste.job.succeeded.is_empty() {
            let clipboard = paste.clipboard;
            let snapshot = paste.snapshot.clone();
            let moved = paste.job.succeeded.clone();
            let cleanup = clipboard.finish_cut(snapshot.clone(), moved.clone(), cx);
            cx.spawn_in(window, async move |this, cx| {
                let clipboard_error = cleanup.await.err().map(|error| error.to_string());
                let _ = this.update_in(cx, |browser, window, cx| {
                    file_clipboard::note_cut_paste(&snapshot, &moved, cx);
                    // Verify the published remainder asynchronously; a replaced
                    // clipboard clears the markers instead of keeping them.
                    file_clipboard::refresh(cx);
                    browser.finish_paste(paste, clipboard_error, ending, window, cx);
                });
            })
            .detach();
            return;
        }
        self.finish_paste(paste, None, ending, window, cx);
    }

    fn cancel_paste(&mut self, paste: Paste, window: &mut Window, cx: &mut Context<Self>) {
        if paste.job.is_untouched() {
            self.operations.in_progress = false;
            self.finish_background(
                notifications::Report::new(
                    notifications::Level::Info,
                    i18n::text("paste-cancelled", cx),
                ),
                cx,
            );
            cx.notify();
            return;
        }
        // Earlier passes may already have moved files; report them instead of
        // implying the whole paste was undone.
        self.complete_paste(paste, Ending::Cancelled, window, cx);
    }

    /// Terminal flow error; a flow that already moved something is reported as
    /// partial completion instead of a plain failure.
    fn abort_paste(
        &mut self,
        mut paste: Paste,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paste.job.is_untouched() {
            self.fail_paste(message, notifications::Level::Error, cx);
            return;
        }
        paste.job.record_abort(message);
        self.complete_paste(paste, Ending::Aborted, window, cx);
    }

    fn fail_paste(&mut self, message: String, level: notifications::Level, cx: &mut Context<Self>) {
        self.operations.in_progress = false;
        self.finish_background(notifications::Report::new(level, message), cx);
        cx.notify();
    }

    fn finish_paste(
        &mut self,
        paste: Paste,
        clipboard_error: Option<String>,
        ending: Ending,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let origin_tab = paste.origin_tab;
        self.operations.in_progress = false;
        let summary = paste.job.into_summary(clipboard_error);
        self.finish_summary(summary, ending, cx);
        Self::restore_file_focus(cx.entity().downgrade(), origin_tab, window);
        cx.notify();
    }

    /// Report a completed transfer and refresh every affected view. Paste and
    /// drag-and-drop share this so their conflict, selection and refresh
    /// behavior stays identical.
    pub(super) fn finish_transfer(
        &mut self,
        report: transfer::Report,
        clipboard_error: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let summary = Summary::from_report(report, clipboard_error);
        self.finish_summary(summary, Ending::Completed, cx);
    }

    fn finish_summary(&mut self, summary: Summary, ending: Ending, cx: &mut Context<Self>) {
        if summary.requires_history_reset() {
            // A move may commit its destination before source removal fails.
            // That incomplete effect cannot be represented by an inverse move.
            self.operations.history.clear();
        }
        let kind = summary.history_kind();
        if summary.mode == transfer::Mode::Move {
            self.clear_completed_selection(&summary.succeeded);
        }
        let changed = summary.affected_paths();
        self.operations
            .history
            .record(history::Entry::new(kind, summary.undo));
        self.refresh_after_file_operation(&changed, cx);
        let failed = summary.failures.len();
        let level = if failed > 0 || summary.clipboard_error.is_some() {
            notifications::Level::Error
        } else if ending == Ending::Completed {
            notifications::Level::Success
        } else {
            notifications::Level::Warning
        };
        let text = match ending {
            Ending::Completed if failed == 0 => i18n::count(
                if summary.mode == transfer::Mode::Move {
                    "move-done"
                } else {
                    "copy-done"
                },
                summary.succeeded.len(),
                cx,
            ),
            Ending::Completed => t!(
                @cx,
                "transfer-partial",
                succeeded = summary.succeeded.len() as i64,
                failed = failed as i64,
            ),
            Ending::Cancelled if summary.succeeded.is_empty() => i18n::text("paste-cancelled", cx),
            Ending::Cancelled => t!(
                @cx,
                "paste-cancelled-partial",
                completed = summary.succeeded.len() as i64,
            ),
            Ending::Aborted => t!(
                @cx,
                "paste-aborted-partial",
                completed = summary.succeeded.len() as i64,
            ),
        };
        let mut notice = notifications::Report::new(level, text);
        for detail in &summary.failures {
            notice = notice.detail(detail.clone());
        }
        if let Some(error) = summary.clipboard_error {
            notice = notice.detail(t!(@cx, "clipboard-update-failed", error));
        }
        self.finish_background(notice, cx);
    }
}
