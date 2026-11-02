//! Paste transfers with pre-flight conflict naming.
//!
//! A paste captures the clipboard snapshot and the destination when it starts,
//! so navigating elsewhere cannot retarget it. Existing or duplicated target
//! names are named interactively before any filesystem work; the transfer
//! itself still commits every entry atomically without replacing anything.
//! Probing the destination and suggesting a free name run on the background
//! executor, so a slow or network mount never blocks the UI.

mod conflict;

use std::{collections::HashSet, ffi::OsString};

use fluent_bundle::FluentArgs;
use gds::{i18n, ui::notifications};

use super::*;
use crate::features::file_operations::{names, transfer};
use crate::shared::lib::clipboard::{self as file_clipboard, Bridge, Mode, Snapshot};
use conflict::ConflictDialog;

/// One paste flow that survives dialogs and transfer passes.
struct Paste {
    origin_tab: u64,
    clipboard: Bridge,
    snapshot: Snapshot,
    destination: PathBuf,
    mode: transfer::Mode,
    /// Sources not yet completed; after a pass only conflicting sources remain.
    pending: Vec<PathBuf>,
    chosen: Vec<names::NamedTarget>,
    succeeded: Vec<PathBuf>,
    committed: Vec<PathBuf>,
    failures: Vec<(PathBuf, String)>,
}

/// Background pre-flight result: commit the flow, ask about one conflict, or
/// stop with the error the destination inspection returned.
enum Inspection {
    Ready(Paste),
    Conflict {
        paste: Paste,
        prompt: ConflictPrompt,
    },
    Failed {
        paste: Paste,
        error: names::Error,
    },
}

/// The conflict to ask the user about, with a free name suggested for it.
struct ConflictPrompt {
    collision: names::Collision,
    suggestion: OsString,
}

impl Paste {
    /// Detect conflicting targets and name the first one.
    ///
    /// Probing the destination can block on a slow or network mount, so this
    /// runs on the background executor; the caller only presents the result.
    fn inspect(self) -> Inspection {
        let collisions = match names::collisions(&self.destination, &self.pending, &self.chosen) {
            Ok(collisions) => collisions,
            Err(error) => return Inspection::Failed { paste: self, error },
        };
        let Some(collision) = collisions.into_iter().next() else {
            return Inspection::Ready(self);
        };
        let reserved = self.reserved_for(&collision.source);
        let suggested = names::free_name(
            &self.destination,
            names::target_name(&collision.source, &self.chosen),
            &reserved,
        );
        match suggested {
            Ok(suggestion) => Inspection::Conflict {
                paste: self,
                prompt: ConflictPrompt {
                    collision,
                    suggestion,
                },
            },
            Err(error) => Inspection::Failed { paste: self, error },
        }
    }

    fn name_for(&self, source: &Path) -> OsString {
        names::target_name(source, &self.chosen).to_owned()
    }

    /// Planned targets of the other pending sources, so two suggestions cannot
    /// both pick the same free leaf.
    fn reserved_for(&self, source: &Path) -> HashSet<OsString> {
        self.pending
            .iter()
            .filter(|pending| pending.as_path() != source)
            .map(|pending| self.name_for(pending.as_path()))
            .collect()
    }

    fn record_name(&mut self, source: PathBuf, name: OsString) {
        self.chosen.retain(|target| target.source != source);
        self.chosen.push(names::NamedTarget::new(source, name));
    }
}

/// Final result of one paste flow, merged across retried conflict passes.
struct Summary {
    mode: transfer::Mode,
    succeeded: Vec<PathBuf>,
    committed: Vec<PathBuf>,
    failures: Vec<String>,
    clipboard_error: Option<String>,
}

impl Summary {
    fn from_report(report: transfer::Report, clipboard_error: Option<String>) -> Self {
        let mode = report.mode;
        let succeeded = report.succeeded_sources();
        let committed = report.committed_destinations();
        let failures = report
            .items
            .iter()
            .filter_map(|item| {
                let Err(error) = &item.result else {
                    return None;
                };
                tracing::warn!(
                    "File transfer failed for {}: {error}",
                    item.source.display()
                );
                let path = item
                    .source
                    .as_os_str()
                    .to_string_lossy()
                    .escape_debug()
                    .to_string();
                Some(format!("{path}: {error}"))
            })
            .collect();
        Self {
            mode,
            succeeded,
            committed,
            failures,
            clipboard_error,
        }
    }
}

impl Paste {
    fn summary(self, clipboard_error: Option<String>) -> Summary {
        Summary {
            mode: self.mode,
            succeeded: self.succeeded,
            committed: self.committed,
            failures: self
                .failures
                .into_iter()
                .map(|(_, detail)| detail)
                .collect(),
            clipboard_error,
        }
    }
}

/// Why a paste flow stopped, which decides how partial work is reported.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    Completed,
    Cancelled,
    Aborted,
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
                    let mut args = fluent_bundle::FluentArgs::new();
                    args.set("error", error.to_string());
                    notifications::error(
                        i18n::format("clipboard-cancel-failed", Some(&args), cx),
                        cx,
                    );
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
    }

    pub(super) fn paste_files(
        &mut self,
        destination: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.in_progress
            || self.settings.open
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
        notifications::Operation::start(i18n::text("paste-working", cx), cx);
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
            let paste = Paste {
                origin_tab,
                mode: match snapshot.mode {
                    Mode::Copy => transfer::Mode::Copy,
                    Mode::Cut => transfer::Mode::Move,
                },
                pending: snapshot.paths.clone(),
                snapshot,
                clipboard,
                destination,
                chosen: Vec::new(),
                succeeded: Vec::new(),
                committed: Vec::new(),
                failures: Vec::new(),
            };
            let _ = this.update_in(cx, |browser, window, cx| {
                if paste
                    .pending
                    .iter()
                    .any(|path| !browser.device_path_available(path, cx))
                    || !browser.device_path_available(&paste.destination, cx)
                {
                    browser.fail_paste(
                        i18n::text("device-path-unavailable", cx).to_string(),
                        notifications::Level::Error,
                        cx,
                    );
                    return;
                }
                browser.operations.paths = paste
                    .pending
                    .iter()
                    .cloned()
                    .chain(std::iter::once(paste.destination.clone()))
                    .collect();
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
        inspection: Inspection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match inspection {
            Inspection::Ready(paste) => self.execute_paste(paste, window, cx),
            Inspection::Failed { paste, error } => {
                self.abort_paste(paste, error.to_string(), window, cx);
            }
            Inspection::Conflict { paste, prompt } => self.ask_conflict(paste, prompt, window, cx),
        }
    }

    /// Show the conflict dialog for one conflict that already has a suggestion.
    fn ask_conflict(
        &mut self,
        paste: Paste,
        prompt: ConflictPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dialog = ConflictDialog::new(cx.entity().downgrade(), paste, prompt, window, cx);
        ConflictDialog::open(dialog, window);
        cx.notify();
    }

    /// Recheck every pending source off the UI thread, then present the next
    /// conflict or commit the flow.
    fn inspect_paste(&mut self, paste: Paste, window: &mut Window, cx: &mut Context<Self>) {
        let work = cx
            .background_executor()
            .spawn(async move { paste.inspect() });
        cx.spawn_in(window, async move |this, cx| {
            let inspection = work.await;
            let _ = this.update_in(cx, |browser, window, cx| {
                browser.present_conflicts(inspection, window, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn execute_paste(&mut self, paste: Paste, window: &mut Window, cx: &mut Context<Self>) {
        if paste
            .pending
            .iter()
            .any(|path| !self.device_path_available(path, cx))
            || !self.device_path_available(&paste.destination, cx)
        {
            self.abort_paste(
                paste,
                i18n::text("device-path-unavailable", cx).to_string(),
                window,
                cx,
            );
            return;
        }
        let count = paste.pending.len();
        let request = transfer::Request {
            paths: paste.pending.clone(),
            destination: paste.destination.clone(),
            mode: paste.mode,
        };
        let batch = match transfer::Batch::named(request, paste.chosen.clone()) {
            Ok(batch) => batch,
            Err(error) => {
                self.abort_paste(paste, error.to_string(), window, cx);
                return;
            }
        };
        notifications::Operation::start(i18n::count("transfer-working", count, cx), cx);
        let work = cx
            .background_executor()
            .spawn(async move { batch.execute() });
        cx.spawn_in(window, async move |this, cx| {
            let report = work.await;
            let _ = this.update_in(cx, |browser, window, cx| {
                browser.finish_pass(paste, report, window, cx);
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
        let mut retry: Vec<PathBuf> = Vec::new();
        for item in report.items {
            let source = item.source.clone();
            paste.pending.retain(|pending| pending != &source);
            match item.result {
                Ok(()) => {
                    paste.chosen.retain(|target| target.source != source);
                    if item.destination_committed {
                        paste.committed.push(item.destination);
                    }
                    paste.succeeded.push(source);
                }
                // The name the user picked for this source stays in force, so
                // the retry is rechecked against that name rather than the
                // original leaf.
                Err(transfer::Error::Conflict { .. }) => retry.push(source),
                Err(error) => {
                    paste.chosen.retain(|target| target.source != source);
                    tracing::warn!("File transfer failed for {}: {error}", source.display());
                    let label = source
                        .as_os_str()
                        .to_string_lossy()
                        .escape_debug()
                        .to_string();
                    if item.destination_committed {
                        paste.committed.push(item.destination);
                    }
                    paste.failures.push((source, format!("{label}: {error}")));
                }
            }
        }
        if retry.is_empty() {
            self.complete_paste(paste, Ending::Completed, window, cx);
            return;
        }
        // A target that appeared after naming is prompted again instead of
        // being replaced; the recheck runs off the UI thread.
        paste.pending = retry;
        self.inspect_paste(paste, window, cx);
    }

    /// Finish a flow, cleaning the clipboard for cut sources that really moved.
    fn complete_paste(
        &mut self,
        paste: Paste,
        ending: Ending,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paste.mode == transfer::Mode::Move && !paste.succeeded.is_empty() {
            let clipboard = paste.clipboard;
            let snapshot = paste.snapshot.clone();
            let moved = paste.succeeded.clone();
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
        if paste.succeeded.is_empty() && paste.failures.is_empty() {
            self.operations.in_progress = false;
            notifications::Operation::finish(
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
        if paste.succeeded.is_empty() && paste.failures.is_empty() {
            self.fail_paste(message, notifications::Level::Error, cx);
            return;
        }
        paste.failures.push((PathBuf::new(), message));
        self.complete_paste(paste, Ending::Aborted, window, cx);
    }

    fn fail_paste(&mut self, message: String, level: notifications::Level, cx: &mut Context<Self>) {
        self.operations.in_progress = false;
        notifications::Operation::finish(notifications::Report::new(level, message), cx);
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
        let summary = paste.summary(clipboard_error);
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
        let changed: Vec<PathBuf> = if summary.mode == transfer::Mode::Move {
            self.clear_completed_selection(&summary.succeeded);
            summary
                .committed
                .iter()
                .chain(summary.succeeded.iter())
                .cloned()
                .collect()
        } else {
            summary.committed.clone()
        };
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
            Ending::Completed => {
                let mut arguments = FluentArgs::new();
                arguments.set("succeeded", summary.succeeded.len() as i64);
                arguments.set("failed", failed as i64);
                i18n::format("transfer-partial", Some(&arguments), cx)
            }
            Ending::Cancelled if summary.succeeded.is_empty() => i18n::text("paste-cancelled", cx),
            Ending::Cancelled => {
                let mut arguments = FluentArgs::new();
                arguments.set("completed", summary.succeeded.len() as i64);
                i18n::format("paste-cancelled-partial", Some(&arguments), cx)
            }
            Ending::Aborted => {
                let mut arguments = FluentArgs::new();
                arguments.set("completed", summary.succeeded.len() as i64);
                i18n::format("paste-aborted-partial", Some(&arguments), cx)
            }
        };
        let mut notice = notifications::Report::new(level, text);
        for detail in &summary.failures {
            notice = notice.detail(detail.clone());
        }
        if let Some(error) = summary.clipboard_error {
            let mut arguments = FluentArgs::new();
            arguments.set("error", error);
            notice = notice.detail(i18n::format(
                "clipboard-update-failed",
                Some(&arguments),
                cx,
            ));
        }
        notifications::Operation::finish(notice, cx);
    }
}
