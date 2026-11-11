//! Session-local file-operation history. Each completed effect supplies its inverse.
//!
//! Inspired by Nautilus' [undo manager](https://gitlab.gnome.org/GNOME/nautilus/-/blob/main/src/nautilus-file-undo-manager.c):
//! replay does not register a new user operation.
//! Batches retain only completed effects, including after partial failures.

use std::{ffi::OsString, path::PathBuf};

use super::{names::NamedTarget, recycle, transfer};

pub fn transfer_steps(report: &transfer::Report) -> Vec<Step> {
    report
        .items
        .iter()
        .filter(|item| {
            item.result.is_ok()
                || (report.mode == transfer::Mode::Copy && item.destination_committed)
        })
        .map(|item| match report.mode {
            transfer::Mode::Copy => Step::trash(item.destination.clone()),
            transfer::Mode::Move => Step::Move {
                source: item.destination.clone(),
                destination: item.source.clone(),
            },
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Copy,
    Move,
    Rename,
    NewFolder,
    Trash,
    Restore,
}

impl Kind {
    pub fn label_id(self) -> &'static str {
        match self {
            Self::Copy => "history-copy",
            Self::Move => "history-move",
            Self::Rename => "history-rename",
            Self::NewFolder => "history-new-folder",
            Self::Trash => "history-trash",
            Self::Restore => "history-restore",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Step {
    Move {
        source: PathBuf,
        destination: PathBuf,
    },
    Trash {
        path: PathBuf,
        previous: Option<OsString>,
    },
    Restore(recycle::Entry),
}

impl Step {
    pub fn trash(path: PathBuf) -> Self {
        Self::Trash {
            path,
            previous: None,
        }
    }

    pub fn undo_restore(entry: &recycle::Entry) -> Self {
        Self::Trash {
            path: entry.original_path.clone(),
            previous: Some(entry.id.clone()),
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        match self {
            Self::Move {
                source,
                destination,
            } => vec![source.clone(), destination.clone()],
            Self::Trash { path, .. } => vec![path.clone()],
            Self::Restore(entry) => vec![entry.original_path.clone()],
        }
    }

    /// # Errors
    /// Propagates cancellation, move, trash or restore failures.
    fn apply(&self, cancel: &tokio_util::sync::CancellationToken) -> Result<Self, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        match self {
            Self::Move {
                source,
                destination,
            } => {
                let parent = destination
                    .parent()
                    .ok_or_else(|| Error::InvalidPath(destination.clone()))?;
                let name = destination
                    .file_name()
                    .ok_or_else(|| Error::InvalidPath(destination.clone()))?;
                let batch = transfer::Batch::named(
                    transfer::Request {
                        paths: vec![source.clone()],
                        destination: parent.to_owned(),
                        mode: transfer::Mode::Move,
                    },
                    vec![NamedTarget::new(source.clone(), name.to_owned())],
                )?;
                batch
                    .execute_cancellable(cancel.clone())
                    .items
                    .pop()
                    .expect("one move result")
                    .result?;
                Ok(Self::Move {
                    source: destination.clone(),
                    destination: source.clone(),
                })
            }
            Self::Trash { path, .. } => Ok(Self::Restore(
                recycle::trash_cancellable(path, cancel)?.entry?,
            )),
            Self::Restore(entry) => {
                recycle::perform_cancellable(recycle::Action::Restore, vec![entry.clone()], cancel)
                    .pop()
                    .expect("one restore result")
                    .result?;
                Ok(Self::undo_restore(entry))
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("operation cancelled")]
    Cancelled,
    #[error("invalid history target: {0}")]
    InvalidPath(PathBuf),
    #[error(transparent)]
    Transfer(#[from] transfer::Error),
    #[error(transparent)]
    Trash(#[from] recycle::Error),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: Kind,
    pub steps: Vec<Step>,
}

struct PendingTrash {
    step: Step,
    previous: Option<OsString>,
    original: PathBuf,
}

impl Entry {
    pub fn new(kind: Kind, steps: Vec<Step>) -> Self {
        Self { kind, steps }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.steps.iter().flat_map(Step::paths).collect()
    }

    pub fn execute_cancellable(self, cancel: &tokio_util::sync::CancellationToken) -> Report {
        let mut report = Report {
            kind: self.kind,
            changed: self.paths(),
            inverse: Vec::new(),
            remaining: Vec::new(),
            errors: Vec::new(),
            replacements: Vec::new(),
        };
        let mut tracker = recycle::Tracker::new();
        let mut pending_trash: Vec<PendingTrash> = Vec::new();
        for step in self.steps.into_iter().rev() {
            if cancel.is_cancelled() {
                if report.remaining.is_empty() {
                    report.errors.push(Error::Cancelled.to_string());
                }
                report.remaining.push(step);
                continue;
            }
            if let Step::Trash { path, previous } = &step {
                match tracker.delete(path, cancel) {
                    Ok(original) => pending_trash.push(PendingTrash {
                        previous: previous.clone(),
                        step,
                        original,
                    }),
                    Err(error) => {
                        report.errors.push(error.to_string());
                        report.remaining.push(step);
                    }
                }
                continue;
            }
            match step.apply(cancel) {
                Ok(inverse) => {
                    report.inverse.push(inverse);
                }
                Err(error) => {
                    report.errors.push(error.to_string());
                    report.remaining.push(step);
                }
            }
        }
        let originals = pending_trash
            .iter()
            .map(|item| item.original.clone())
            .collect();
        for (pending, receipt) in pending_trash.into_iter().zip(tracker.finish(originals)) {
            match receipt {
                Ok(current) => {
                    if let Some(previous) = pending.previous {
                        report.replacements.push(Replacement {
                            previous,
                            current: current.clone(),
                        });
                    }
                    report.inverse.push(Step::Restore(current));
                }
                Err(error) => {
                    report.errors.push(error.to_string());
                    report.remaining.push(pending.step);
                }
            }
        }
        report.remaining.reverse();
        report
    }
}

struct Replacement {
    previous: OsString,
    current: recycle::Entry,
}

pub struct Report {
    pub kind: Kind,
    pub changed: Vec<PathBuf>,
    pub inverse: Vec<Step>,
    pub remaining: Vec<Step>,
    pub errors: Vec<String>,
    replacements: Vec<Replacement>,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl History {
    pub fn record(&mut self, entry: Entry) {
        if !entry.steps.is_empty() {
            self.redo.clear();
            self.undo.push(entry);
        }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn peek(&self, direction: Direction) -> Option<&Entry> {
        match direction {
            Direction::Undo => self.undo.last(),
            Direction::Redo => self.redo.last(),
        }
    }

    pub fn take(&mut self, direction: Direction) -> Option<Entry> {
        self.stack(direction).pop()
    }

    pub fn finish(&mut self, direction: Direction, report: &mut Report) {
        // Re-trashing a restored file creates a new platform id. Rebind only
        // that exact old record, preserving unrelated same-path deletions.
        for replacement in &report.replacements {
            for step in self
                .undo
                .iter_mut()
                .chain(self.redo.iter_mut())
                .flat_map(|entry| &mut entry.steps)
                .chain(report.remaining.iter_mut())
                .chain(report.inverse.iter_mut())
            {
                match step {
                    Step::Restore(entry) if entry.id == replacement.previous => {
                        *entry = replacement.current.clone()
                    }
                    Step::Trash {
                        previous: Some(id), ..
                    } if *id == replacement.previous => *id = replacement.current.id.clone(),
                    _ => {}
                }
            }
        }
        if !report.remaining.is_empty() {
            self.stack(direction).push(Entry::new(
                report.kind,
                std::mem::take(&mut report.remaining),
            ));
        }
        if !report.inverse.is_empty() {
            let opposite = match direction {
                Direction::Undo => Direction::Redo,
                Direction::Redo => Direction::Undo,
            };
            self.stack(opposite)
                .push(Entry::new(report.kind, std::mem::take(&mut report.inverse)));
        }
    }

    fn stack(&mut self, direction: Direction) -> &mut Vec<Entry> {
        match direction {
            Direction::Undo => &mut self.undo,
            Direction::Redo => &mut self.redo,
        }
    }
}
