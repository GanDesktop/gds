//! Multi-pass paste state machine, conflict detection, and transfer summary.
//!
//! Existing or duplicated target names are resolved interactively before each
//! transfer pass; if a new target appears on disk while a pass is running, only
//! the newly conflicting source is queued for a retry pass while preserving the
//! user's chosen leaf name.

use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
};

use super::{history, names, transfer};

/// Pure domain state for one multi-pass paste operation.
pub(crate) struct PasteJob {
    pub destination: PathBuf,
    pub mode: transfer::Mode,
    /// Sources not yet completed; after a pass only conflicting sources remain.
    pub pending: Vec<PathBuf>,
    pub chosen: Vec<names::NamedTarget>,
    pub succeeded: Vec<PathBuf>,
    pub undo: Vec<history::Step>,
    pub committed: Vec<PathBuf>,
    pub failures: Vec<String>,
}

/// Background pre-flight result: commit the flow, ask about one conflict, or
/// stop with the error the destination inspection returned.
pub(crate) enum Inspection {
    Ready(PasteJob),
    Conflict {
        job: PasteJob,
        conflict: PasteConflict,
    },
    Failed {
        job: PasteJob,
        error: names::Error,
    },
}

impl Inspection {
    pub(crate) fn into_job(self) -> PasteJob {
        match self {
            Self::Ready(job) | Self::Conflict { job, .. } | Self::Failed { job, .. } => job,
        }
    }
}

/// The conflict to ask the user about, with a free name suggested for it.
pub(crate) struct PasteConflict {
    pub collision: names::Collision,
    pub suggestion: OsString,
}

/// Outcome of merging one completed transfer pass into a [`PasteJob`].
pub(crate) enum PassOutcome {
    Completed(PasteJob),
    RetryConflict(PasteJob),
}

/// Why a paste flow stopped, which decides how partial work is reported.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ending {
    Completed,
    Cancelled,
    Aborted,
}

/// Final result of one paste or transfer flow, merged across retried passes.
pub(crate) struct Summary {
    pub mode: transfer::Mode,
    pub succeeded: Vec<PathBuf>,
    pub undo: Vec<history::Step>,
    pub committed: Vec<PathBuf>,
    pub failures: Vec<String>,
    pub clipboard_error: Option<String>,
}

impl Summary {
    pub(crate) fn from_report(report: transfer::Report, clipboard_error: Option<String>) -> Self {
        let undo = history::transfer_steps(&report);
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
            undo,
            committed,
            failures,
            clipboard_error,
        }
    }

    pub(crate) fn requires_history_reset(&self) -> bool {
        self.committed.len() > self.undo.len()
    }

    pub(crate) fn history_kind(&self) -> history::Kind {
        if self.mode == transfer::Mode::Move {
            history::Kind::Move
        } else {
            history::Kind::Copy
        }
    }

    pub(crate) fn affected_paths(&self) -> Vec<PathBuf> {
        if self.mode == transfer::Mode::Move {
            self.committed
                .iter()
                .chain(self.succeeded.iter())
                .cloned()
                .collect()
        } else {
            self.committed.clone()
        }
    }
}

impl PasteJob {
    pub(crate) fn new(destination: PathBuf, mode: transfer::Mode, pending: Vec<PathBuf>) -> Self {
        Self {
            destination,
            mode,
            pending,
            chosen: Vec::new(),
            succeeded: Vec::new(),
            undo: Vec::new(),
            committed: Vec::new(),
            failures: Vec::new(),
        }
    }

    pub(crate) fn take(&mut self) -> Self {
        let mode = self.mode;
        std::mem::replace(self, Self::new(PathBuf::new(), mode, Vec::new()))
    }

    pub(crate) fn involved_paths(&self) -> Vec<PathBuf> {
        self.pending
            .iter()
            .cloned()
            .chain(std::iter::once(self.destination.clone()))
            .collect()
    }

    pub(crate) fn is_untouched(&self) -> bool {
        self.succeeded.is_empty() && self.failures.is_empty()
    }

    /// Detect conflicting targets and suggest a free name for the first one.
    pub(crate) fn inspect(self) -> Inspection {
        let collisions = match names::collisions(&self.destination, &self.pending, &self.chosen) {
            Ok(collisions) => collisions,
            Err(error) => return Inspection::Failed { job: self, error },
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
                job: self,
                conflict: PasteConflict {
                    collision,
                    suggestion,
                },
            },
            Err(error) => Inspection::Failed { job: self, error },
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

    pub(crate) fn resolve_conflict(&mut self, source: PathBuf, name: OsString) {
        self.chosen.retain(|target| target.source != source);
        self.chosen.push(names::NamedTarget::new(source, name));
    }

    /// Build the next transfer batch from the remaining pending sources and
    /// any user-chosen conflict names.
    ///
    /// # Errors
    /// Propagates batch validation errors from [`transfer::Batch::named`].
    pub(crate) fn prepare_batch(&self) -> Result<transfer::Batch, transfer::Error> {
        let request = transfer::Request {
            paths: self.pending.clone(),
            destination: self.destination.clone(),
            mode: self.mode,
        };
        transfer::Batch::named(request, self.chosen.clone())
    }

    /// Merge one completed pass; a conflict that appeared after naming is
    /// queued for another inspection pass instead of overwriting the target.
    pub(crate) fn apply_pass(mut self, report: transfer::Report) -> PassOutcome {
        let mut retry: Vec<PathBuf> = Vec::new();
        self.undo.extend(history::transfer_steps(&report));
        for item in report.items {
            let source = item.source.clone();
            self.pending.retain(|pending| pending != &source);
            match item.result {
                Ok(()) => {
                    self.chosen.retain(|target| target.source != source);
                    if item.destination_committed {
                        self.committed.push(item.destination);
                    }
                    self.succeeded.push(source);
                }
                // The name the user picked for this source stays in force, so
                // the retry is rechecked against that name rather than the
                // original leaf.
                Err(transfer::Error::Conflict { .. }) => retry.push(source),
                Err(error) => {
                    self.chosen.retain(|target| target.source != source);
                    tracing::warn!("File transfer failed for {}: {error}", source.display());
                    let label = source
                        .as_os_str()
                        .to_string_lossy()
                        .escape_debug()
                        .to_string();
                    if item.destination_committed {
                        self.committed.push(item.destination);
                    }
                    self.failures.push(format!("{label}: {error}"));
                }
            }
        }
        if retry.is_empty() {
            PassOutcome::Completed(self)
        } else {
            self.pending = retry;
            PassOutcome::RetryConflict(self)
        }
    }

    pub(crate) fn record_abort(&mut self, detail: String) {
        self.failures.push(detail);
    }

    pub(crate) fn into_summary(self, clipboard_error: Option<String>) -> Summary {
        Summary {
            mode: self.mode,
            succeeded: self.succeeded,
            undo: self.undo,
            committed: self.committed,
            failures: self.failures,
            clipboard_error,
        }
    }
}
