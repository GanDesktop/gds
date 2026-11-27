//! Multi-pass paste state machine, conflict classification, folder merge expansion, and summary.
//!
//! Each planned transfer item carries its own destination directory, target leaf name, and
//! collision policy so directory merges can expand depth-first into the same queue without
//! blocking background copy loops on UI callbacks.

use std::{
    collections::{HashSet, VecDeque},
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::{history, names, transfer};

/// Cap direct-child counting when building directory conflict summaries so large folders stay fast.
const CONFLICT_DIR_ITEM_LIMIT: usize = 1_000;

/// Entry kind captured via `symlink_metadata` (never following symlinks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

impl EntryKind {
    fn of(file_type: fs::FileType) -> Self {
        if file_type.is_symlink() {
            Self::Symlink
        } else if file_type.is_dir() {
            Self::Directory
        } else if file_type.is_file() {
            Self::File
        } else {
            Self::Other
        }
    }
}

/// Bounded direct-child count for a conflicting directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChildItems {
    pub count: usize,
    pub truncated: bool,
}

/// Metadata snapshot shown side-by-side in the conflict resolution dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntryInfo {
    pub path: PathBuf,
    pub name: OsString,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub items: Option<ChildItems>,
    pub modified: Option<SystemTime>,
}

/// Structural classification of a name collision at the destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConflictKind {
    /// Both source and destination are real directories (not symlinks): supports Merge.
    DirectoryIntoDirectory,
    /// Both source and destination are non-directories (files or symlinks): supports Replace.
    FileIntoFile,
    /// One side is a real directory and the other is not: neither Merge nor Replace is allowed.
    TypeMismatch,
    /// Target collides with the source itself (e.g. user typed original name back) or another item
    /// already staged in the same batch: neither Merge nor Replace is allowed.
    SameEntry,
}

/// User action chosen for one conflict prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConflictAction {
    /// Atomically overwrite the existing non-directory destination.
    Replace,
    /// Recursively merge the source directory into the existing destination directory.
    Merge,
    /// Transfer the source under a new leaf name.
    Rename(OsString),
    /// Skip this conflicting source and leave the existing destination untouched.
    Skip,
}

/// Decision returned by the conflict dialog, optionally applying to subsequent conflicts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConflictResolution {
    pub action: ConflictAction,
    pub apply_to_all: bool,
}

/// One concrete transfer item queued inside a [`PasteJob`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TransferTask {
    pub source: PathBuf,
    pub destination_dir: PathBuf,
    pub target_name: OsString,
    pub policy: transfer::CollisionPolicy,
    pub user_renamed: bool,
    pub expand_merge: bool,
    pub root_source: PathBuf,
}

/// The conflict to ask the user about, with metadata comparison, destination context, live
/// collision names, and a free-name suggestion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PasteConflict {
    pub task: TransferTask,
    pub kind: ConflictKind,
    pub source: EntryInfo,
    pub existing: EntryInfo,
    pub suggestion: OsString,
    /// Relative directory under the batch root destination (non-empty during nested folder merge).
    pub relative_dir: Option<PathBuf>,
    /// Names already occupied on disk or planned in `task.destination_dir`, for live rename checks.
    pub occupied_names: HashSet<OsString>,
    /// Number of queued items still waiting after this conflict.
    pub remaining_after_current: usize,
    pub has_more_conflicts_possible: bool,
}

impl PasteConflict {
    /// Display form of the conflicting leaf name, safe for dialog text.
    pub(crate) fn target_label(&self) -> String {
        self.task
            .target_name
            .to_string_lossy()
            .escape_debug()
            .to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileAllPolicy {
    Replace,
    Skip,
    AutoRename,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirAllPolicy {
    Merge,
    Skip,
    AutoRename,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MismatchAllPolicy {
    Skip,
    AutoRename,
}

/// Pure domain state for one multi-pass paste/drop operation.
pub(crate) struct PasteJob {
    pub destination: PathBuf,
    pub mode: transfer::Mode,
    queue: VecDeque<TransferTask>,
    ready: Vec<TransferTask>,
    in_flight: Vec<TransferTask>,
    cleanup_dirs: Vec<PathBuf>,
    merged_root_sources: HashSet<PathBuf>,
    touched_dirs: Vec<PathBuf>,
    file_all: Option<FileAllPolicy>,
    dir_all: Option<DirAllPolicy>,
    mismatch_all: Option<MismatchAllPolicy>,
    succeeded: Vec<PathBuf>,
    cut_completed: Vec<PathBuf>,
    undo: Vec<history::Step>,
    committed: Vec<PathBuf>,
    failures: Vec<String>,
    skipped: usize,
    merged_dirs: usize,
    non_undoable: bool,
}

/// Background pre-flight result: execute ready transfers, complete with no remaining disk transfers
/// (e.g. all skipped or empty merged directories), prompt the user for one conflict, or stop on a
/// filesystem inspection error.
pub(crate) enum Inspection {
    Ready(PasteJob),
    Done(PasteJob),
    Conflict {
        job: PasteJob,
        conflict: PasteConflict,
    },
    Failed {
        job: PasteJob,
        error: names::Error,
    },
}

/// Outcome of merging one completed transfer pass into a [`PasteJob`].
pub(crate) enum PassOutcome {
    Completed(PasteJob),
    RetryConflict(PasteJob),
}

/// Final result of one paste or transfer flow, merged across retried passes.
pub(crate) struct Summary {
    pub mode: transfer::Mode,
    pub succeeded: Vec<PathBuf>,
    #[cfg(test)]
    pub cut_completed: Vec<PathBuf>,
    pub undo: Vec<history::Step>,
    pub committed: Vec<PathBuf>,
    pub touched_dirs: Vec<PathBuf>,
    pub failures: Vec<String>,
    pub skipped: usize,
    pub merged_dirs: usize,
    pub non_undoable: bool,
    pub clipboard_error: Option<String>,
}

impl Summary {
    pub(crate) fn requires_history_reset(&self) -> bool {
        self.non_undoable || self.committed.len() > self.undo.len()
    }

    pub(crate) fn history_kind(&self) -> history::Kind {
        if self.mode == transfer::Mode::Move {
            history::Kind::Move
        } else {
            history::Kind::Copy
        }
    }
}

impl PasteJob {
    pub(crate) fn new(destination: PathBuf, mode: transfer::Mode, pending: Vec<PathBuf>) -> Self {
        let mut seen = HashSet::new();
        let queue = pending
            .into_iter()
            .filter(|path| seen.insert(path.clone()))
            .map(|source| {
                let target_name = source
                    .file_name()
                    .map_or_else(|| source.as_os_str().to_owned(), OsString::from);
                TransferTask {
                    root_source: source.clone(),
                    source,
                    destination_dir: destination.clone(),
                    target_name,
                    policy: transfer::CollisionPolicy::Fail,
                    user_renamed: false,
                    expand_merge: false,
                }
            })
            .collect();
        Self {
            destination,
            mode,
            queue,
            ready: Vec::new(),
            in_flight: Vec::new(),
            cleanup_dirs: Vec::new(),
            merged_root_sources: HashSet::new(),
            touched_dirs: Vec::new(),
            file_all: None,
            dir_all: None,
            mismatch_all: None,
            succeeded: Vec::new(),
            cut_completed: Vec::new(),
            undo: Vec::new(),
            committed: Vec::new(),
            failures: Vec::new(),
            skipped: 0,
            merged_dirs: 0,
            non_undoable: false,
        }
    }

    /// True if at least one item has already written or failed on disk (in-memory merge expansion
    /// alone does not count as partial disk progress).
    pub(crate) fn has_disk_progress(&self) -> bool {
        !self.succeeded.is_empty() || !self.committed.is_empty() || !self.failures.is_empty()
    }

    /// True if any item has already completed or failed during earlier passes.
    pub(crate) fn has_recorded_outcomes(&self) -> bool {
        !self.succeeded.is_empty() || !self.failures.is_empty()
    }

    /// Root source paths whose move completed cleanly and can be cleared from a cut clipboard.
    pub(crate) fn completed_cut_paths(&self) -> Option<Vec<PathBuf>> {
        (!self.cut_completed.is_empty()).then(|| self.cut_completed.clone())
    }

    /// Discard all remaining unexecuted tasks (used when the user aborts a conflict dialog).
    pub(crate) fn abort_remaining(&mut self) {
        self.queue.clear();
        self.ready.clear();
    }

    /// Inspect queued items off the UI thread, auto-renaming same-directory copies, expanding
    /// approved folder merges, applying active batch policies, and pausing on the first unresolved
    /// conflict.
    pub(crate) fn inspect(mut self) -> Inspection {
        while let Some(mut task) = self.queue.pop_front() {
            if task.expand_merge {
                let dest_path = task.destination_dir.join(&task.target_name);
                if let Err(error) = self.expand_merge(&task.source, &dest_path, &task.root_source) {
                    return Inspection::Failed { job: self, error };
                }
                continue;
            }

            let is_same_path =
                match same_path(&task.source, &task.destination_dir, &task.target_name) {
                    Ok(same) => same,
                    Err(error) => return Inspection::Failed { job: self, error },
                };

            // Same-directory Copy automatically picks the next free numbered name without prompting.
            if is_same_path && self.mode == transfer::Mode::Copy && !task.user_renamed {
                let reserved = self.reserved_names_in(&task.destination_dir);
                match names::free_name(&task.destination_dir, &task.target_name, &reserved) {
                    Ok(free) => {
                        task.target_name = free;
                        self.ready.push(task);
                        continue;
                    }
                    Err(error) => return Inspection::Failed { job: self, error },
                }
            }

            // Same-directory Move is a silent no-op (leave the entry in place).
            if is_same_path && self.mode == transfer::Mode::Move && !task.user_renamed {
                self.skipped += 1;
                continue;
            }

            let planned_duplicate =
                self.ready_conflicts_with(&task.destination_dir, &task.target_name);
            let dest_path = task.destination_dir.join(&task.target_name);
            let dest_exists = match path_exists(&dest_path) {
                Ok(exists) => exists,
                Err(error) => return Inspection::Failed { job: self, error },
            };

            if !planned_duplicate && !dest_exists {
                self.ready.push(task);
                continue;
            }

            let source_info = match inspect_entry_info(&task.source) {
                Ok(info) => info,
                Err(error) => return Inspection::Failed { job: self, error },
            };

            // Refuse moving/copying/merging a directory into itself or any descendant before
            // prompting or expanding.
            if source_info.kind == EntryKind::Directory
                && let Err(error) = transfer::check_into_self(&task.source, &task.destination_dir)
            {
                let label = task
                    .source
                    .as_os_str()
                    .to_string_lossy()
                    .escape_debug()
                    .to_string();
                self.failures.push(format!("{label}: {error}"));
                continue;
            }

            let existing_info = if dest_exists {
                match inspect_entry_info(&dest_path) {
                    Ok(info) => info,
                    Err(error) => return Inspection::Failed { job: self, error },
                }
            } else {
                match self.ready_entry_info(&task.destination_dir, &task.target_name) {
                    Ok(info) => info,
                    Err(error) => return Inspection::Failed { job: self, error },
                }
            };

            let kind = if is_same_path || planned_duplicate {
                ConflictKind::SameEntry
            } else {
                match (source_info.kind, existing_info.kind) {
                    (EntryKind::Directory, EntryKind::Directory) => {
                        ConflictKind::DirectoryIntoDirectory
                    }
                    (EntryKind::Directory, _) | (_, EntryKind::Directory) => {
                        ConflictKind::TypeMismatch
                    }
                    _ => ConflictKind::FileIntoFile,
                }
            };

            // Honor any active "Apply to all" policy matching this conflict kind.
            match kind {
                ConflictKind::DirectoryIntoDirectory => match self.dir_all {
                    Some(DirAllPolicy::Merge) => {
                        if let Err(error) =
                            self.expand_merge(&task.source, &dest_path, &task.root_source)
                        {
                            return Inspection::Failed { job: self, error };
                        }
                        continue;
                    }
                    Some(DirAllPolicy::Skip) => {
                        self.skipped += 1;
                        continue;
                    }
                    Some(DirAllPolicy::AutoRename) => {
                        let reserved = self.reserved_names_in(&task.destination_dir);
                        match names::free_name(&task.destination_dir, &task.target_name, &reserved)
                        {
                            Ok(free) => {
                                task.target_name = free;
                                task.policy = transfer::CollisionPolicy::Fail;
                                self.ready.push(task);
                                continue;
                            }
                            Err(error) => return Inspection::Failed { job: self, error },
                        }
                    }
                    None => {}
                },
                ConflictKind::FileIntoFile => match self.file_all {
                    Some(FileAllPolicy::Replace) => {
                        task.policy = transfer::CollisionPolicy::ReplaceNonDirectory;
                        self.non_undoable = true;
                        self.ready.push(task);
                        continue;
                    }
                    Some(FileAllPolicy::Skip) => {
                        self.skipped += 1;
                        continue;
                    }
                    Some(FileAllPolicy::AutoRename) => {
                        let reserved = self.reserved_names_in(&task.destination_dir);
                        match names::free_name(&task.destination_dir, &task.target_name, &reserved)
                        {
                            Ok(free) => {
                                task.target_name = free;
                                task.policy = transfer::CollisionPolicy::Fail;
                                self.ready.push(task);
                                continue;
                            }
                            Err(error) => return Inspection::Failed { job: self, error },
                        }
                    }
                    None => {}
                },
                ConflictKind::TypeMismatch | ConflictKind::SameEntry => match self.mismatch_all {
                    Some(MismatchAllPolicy::Skip) => {
                        self.skipped += 1;
                        continue;
                    }
                    Some(MismatchAllPolicy::AutoRename) => {
                        let reserved = self.reserved_names_in(&task.destination_dir);
                        match names::free_name(&task.destination_dir, &task.target_name, &reserved)
                        {
                            Ok(free) => {
                                task.target_name = free;
                                task.policy = transfer::CollisionPolicy::Fail;
                                self.ready.push(task);
                                continue;
                            }
                            Err(error) => return Inspection::Failed { job: self, error },
                        }
                    }
                    None => {}
                },
            }

            let reserved = self.reserved_names_in(&task.destination_dir);
            let suggestion =
                match names::free_name(&task.destination_dir, &task.target_name, &reserved) {
                    Ok(suggestion) => suggestion,
                    Err(error) => return Inspection::Failed { job: self, error },
                };
            let mut occupied_names = list_directory_names(&task.destination_dir);
            occupied_names.extend(reserved);
            occupied_names.insert(task.target_name.clone());
            let relative_dir = task
                .destination_dir
                .strip_prefix(&self.destination)
                .ok()
                .filter(|rel| !rel.as_os_str().is_empty())
                .map(Path::to_path_buf);
            let remaining_after_current = self.queue.len();
            let has_more_conflicts_possible =
                remaining_after_current > 0 || kind == ConflictKind::DirectoryIntoDirectory;
            return Inspection::Conflict {
                job: self,
                conflict: PasteConflict {
                    task,
                    kind,
                    source: source_info,
                    existing: existing_info,
                    suggestion,
                    relative_dir,
                    occupied_names,
                    remaining_after_current,
                    has_more_conflicts_possible,
                },
            };
        }

        if self.ready.is_empty() {
            self.cleanup_merged_sources();
            Inspection::Done(self)
        } else {
            Inspection::Ready(self)
        }
    }

    /// Apply the user's choice for the paused conflict without performing blocking filesystem I/O.
    /// The next [`Self::inspect`] pass runs off the UI thread to expand merges or re-check names.
    pub(crate) fn resolve_conflict(
        &mut self,
        conflict: PasteConflict,
        resolution: ConflictResolution,
    ) {
        let mut task = conflict.task;
        match resolution.action {
            ConflictAction::Replace if conflict.kind == ConflictKind::FileIntoFile => {
                task.policy = transfer::CollisionPolicy::ReplaceNonDirectory;
                self.non_undoable = true;
                self.ready.push(task);
                if resolution.apply_to_all {
                    self.file_all = Some(FileAllPolicy::Replace);
                }
            }
            ConflictAction::Merge if conflict.kind == ConflictKind::DirectoryIntoDirectory => {
                task.expand_merge = true;
                self.queue.push_front(task);
                if resolution.apply_to_all {
                    self.dir_all = Some(DirAllPolicy::Merge);
                }
            }
            ConflictAction::Rename(name) => {
                task.target_name = name;
                task.policy = transfer::CollisionPolicy::Fail;
                task.user_renamed = true;
                task.expand_merge = false;
                self.queue.push_front(task);
                if resolution.apply_to_all {
                    match conflict.kind {
                        ConflictKind::FileIntoFile => {
                            self.file_all = Some(FileAllPolicy::AutoRename);
                        }
                        ConflictKind::DirectoryIntoDirectory => {
                            self.dir_all = Some(DirAllPolicy::AutoRename);
                        }
                        ConflictKind::TypeMismatch | ConflictKind::SameEntry => {
                            self.mismatch_all = Some(MismatchAllPolicy::AutoRename);
                        }
                    }
                }
            }
            ConflictAction::Skip => {
                self.skipped += 1;
                if resolution.apply_to_all {
                    match conflict.kind {
                        ConflictKind::FileIntoFile => {
                            self.file_all = Some(FileAllPolicy::Skip);
                        }
                        ConflictKind::DirectoryIntoDirectory => {
                            self.dir_all = Some(DirAllPolicy::Skip);
                        }
                        ConflictKind::TypeMismatch | ConflictKind::SameEntry => {
                            self.mismatch_all = Some(MismatchAllPolicy::Skip);
                        }
                    }
                }
            }
            // Defensive fallback if an inapplicable action was supplied for this conflict kind.
            ConflictAction::Replace | ConflictAction::Merge => {
                self.queue.push_front(task);
            }
        }
    }

    fn expand_merge(
        &mut self,
        source_dir: &Path,
        dest_dir: &Path,
        root_source: &Path,
    ) -> Result<(), names::Error> {
        let mut children: Vec<OsString> = Vec::new();
        let entries = fs::read_dir(source_dir).map_err(|source| names::Error::Filesystem {
            path: source_dir.to_owned(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| names::Error::Filesystem {
                path: source_dir.to_owned(),
                source,
            })?;
            children.push(entry.file_name());
        }
        children.sort();

        self.non_undoable = true;
        self.merged_dirs += 1;
        self.touched_dirs.push(dest_dir.to_owned());
        if self.mode == transfer::Mode::Move {
            self.cleanup_dirs.push(source_dir.to_owned());
            if source_dir == root_source {
                self.merged_root_sources.insert(source_dir.to_owned());
            }
        }

        for child_name in children.into_iter().rev() {
            let child_source = source_dir.join(&child_name);
            self.queue.push_front(TransferTask {
                source: child_source,
                destination_dir: dest_dir.to_owned(),
                target_name: child_name,
                policy: transfer::CollisionPolicy::Fail,
                user_renamed: false,
                expand_merge: false,
                root_source: root_source.to_owned(),
            });
        }
        Ok(())
    }

    fn ready_conflicts_with(&self, dir: &Path, name: &OsString) -> bool {
        self.ready
            .iter()
            .any(|item| item.destination_dir == dir && &item.target_name == name)
    }

    fn ready_entry_info(&self, dir: &Path, name: &OsString) -> Result<EntryInfo, names::Error> {
        let existing = self
            .ready
            .iter()
            .find(|item| item.destination_dir == dir && &item.target_name == name);
        match existing {
            Some(item) => inspect_entry_info(&item.source),
            None => Ok(EntryInfo {
                path: dir.join(name),
                name: name.clone(),
                kind: EntryKind::Other,
                size: None,
                items: None,
                modified: None,
            }),
        }
    }

    /// Planned target names in `dir` across already-ready and still-queued tasks.
    fn reserved_names_in(&self, dir: &Path) -> HashSet<OsString> {
        self.ready
            .iter()
            .chain(self.queue.iter())
            .filter(|task| task.destination_dir == dir)
            .map(|task| task.target_name.clone())
            .collect()
    }

    /// Build the next transfer batch from all ready tasks.
    ///
    /// # Errors
    /// Propagates batch validation errors from [`transfer::Batch::planned`].
    pub(crate) fn prepare_batch(&mut self) -> Result<transfer::Batch, transfer::Error> {
        let entries: Vec<transfer::PlannedEntry> = self
            .ready
            .iter()
            .map(|task| transfer::PlannedEntry {
                source: task.source.clone(),
                destination_dir: task.destination_dir.clone(),
                target_name: task.target_name.clone(),
                policy: task.policy,
            })
            .collect();
        let batch = transfer::Batch::planned(self.mode, entries)?;
        self.in_flight = std::mem::take(&mut self.ready);
        Ok(batch)
    }

    /// Merge one completed pass; a conflict that appeared after inspection is queued for another
    /// inspection pass instead of silently overwriting the target.
    pub(crate) fn apply_pass(mut self, report: transfer::Report) -> PassOutcome {
        let in_flight = std::mem::take(&mut self.in_flight);
        let mut retry: Vec<TransferTask> = Vec::new();
        self.undo.extend(history::transfer_steps(&report));
        for (task, item) in in_flight.into_iter().zip(report.items) {
            if item.replaced {
                self.non_undoable = true;
            }
            match item.result {
                Ok(()) => {
                    if item.destination_committed {
                        self.committed.push(item.destination);
                    }
                    if task.source == task.root_source {
                        self.cut_completed.push(task.root_source);
                    }
                    self.succeeded.push(item.source);
                }
                Err(transfer::Error::Conflict { .. }) => {
                    let mut retry_task = task;
                    retry_task.policy = transfer::CollisionPolicy::Fail;
                    retry.push(retry_task);
                }
                Err(error) => {
                    tracing::warn!(
                        "File transfer failed for {}: {error}",
                        item.source.display()
                    );
                    let label = item
                        .source
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
        if retry.is_empty() && self.queue.is_empty() {
            PassOutcome::Completed(self)
        } else {
            for task in retry.into_iter().rev() {
                self.queue.push_front(task);
            }
            PassOutcome::RetryConflict(self)
        }
    }

    /// After a Move merge completes, removes now-empty merged source directories bottom-up without
    /// ever deleting skipped or concurrently added children.
    pub(crate) fn cleanup_merged_sources(&mut self) {
        if self.mode != transfer::Mode::Move || self.cleanup_dirs.is_empty() {
            return;
        }
        let dirs = std::mem::take(&mut self.cleanup_dirs);
        for dir in dirs.into_iter().rev() {
            match transfer::remove_empty_dir_if_empty(&dir) {
                Ok(true) => {
                    if let Some(parent) = dir.parent() {
                        self.touched_dirs.push(parent.to_owned());
                    }
                    self.touched_dirs.push(dir.clone());
                    if self.merged_root_sources.remove(&dir) {
                        self.cut_completed.push(dir);
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    let label = dir.as_os_str().to_string_lossy().escape_debug().to_string();
                    self.failures.push(format!("{label}: {error}"));
                }
            }
        }
    }

    pub(crate) fn record_abort(&mut self, detail: String) {
        self.failures.push(detail);
    }

    pub(crate) fn into_summary(self, clipboard_error: Option<String>) -> Summary {
        Summary {
            mode: self.mode,
            succeeded: self.succeeded,
            #[cfg(test)]
            cut_completed: self.cut_completed,
            undo: self.undo,
            committed: self.committed,
            touched_dirs: self.touched_dirs,
            failures: self.failures,
            skipped: self.skipped,
            merged_dirs: self.merged_dirs,
            non_undoable: self.non_undoable,
            clipboard_error,
        }
    }
}

fn path_exists(path: &Path) -> Result<bool, names::Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(names::Error::Filesystem {
            path: path.to_owned(),
            source,
        }),
    }
}

fn same_path(
    source: &Path,
    destination_dir: &Path,
    target_name: &OsString,
) -> Result<bool, names::Error> {
    let Some(source_leaf) = source.file_name() else {
        return Ok(false);
    };
    if source_leaf != target_name.as_os_str() {
        return Ok(false);
    }
    let Some(source_parent) = source.parent() else {
        return Ok(false);
    };
    if source_parent == destination_dir {
        return Ok(true);
    }
    let canonical_source_parent =
        source_parent
            .canonicalize()
            .map_err(|error| names::Error::Filesystem {
                path: source_parent.to_owned(),
                source: error,
            })?;
    let canonical_dest_dir =
        destination_dir
            .canonicalize()
            .map_err(|error| names::Error::Filesystem {
                path: destination_dir.to_owned(),
                source: error,
            })?;
    Ok(canonical_source_parent == canonical_dest_dir)
}

fn inspect_entry_info(path: &Path) -> Result<EntryInfo, names::Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| names::Error::Filesystem {
        path: path.to_owned(),
        source,
    })?;
    let kind = EntryKind::of(metadata.file_type());
    let (size, items) = match kind {
        EntryKind::Directory => (None, count_direct_children(path)),
        EntryKind::File | EntryKind::Symlink | EntryKind::Other => (Some(metadata.len()), None),
    };
    let name = path
        .file_name()
        .map_or_else(|| path.as_os_str().to_owned(), OsString::from);
    Ok(EntryInfo {
        path: path.to_owned(),
        name,
        kind,
        size,
        items,
        modified: metadata.modified().ok(),
    })
}

fn list_directory_names(dir: &Path) -> HashSet<OsString> {
    let Ok(entries) = fs::read_dir(dir) else {
        return HashSet::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect()
}

fn count_direct_children(dir: &Path) -> Option<ChildItems> {
    let entries = fs::read_dir(dir).ok()?;
    let mut count = 0usize;
    let mut truncated = false;
    for entry in entries {
        if entry.is_err() {
            continue;
        }
        if count >= CONFLICT_DIR_ITEM_LIMIT {
            truncated = true;
            break;
        }
        count += 1;
    }
    Some(ChildItems { count, truncated })
}

#[cfg(all(test, any(target_os = "linux", target_os = "freebsd")))]
mod tests {
    use std::{ffi::OsString, fs, os::unix::fs::symlink};

    use super::{
        ConflictAction, ConflictKind, ConflictResolution, Inspection, PassOutcome, PasteJob,
        transfer,
    };

    fn run_to_completion(mut job: PasteJob) -> super::Summary {
        loop {
            match job.inspect() {
                Inspection::Ready(mut ready) => {
                    let batch = ready.prepare_batch().expect("prepare batch");
                    let report = batch.execute();
                    match ready.apply_pass(report) {
                        PassOutcome::Completed(mut done) => {
                            done.cleanup_merged_sources();
                            return done.into_summary(None);
                        }
                        PassOutcome::RetryConflict(next) => job = next,
                    }
                }
                Inspection::Done(done) => return done.into_summary(None),
                Inspection::Conflict { .. } => panic!("unexpected unresolved conflict"),
                Inspection::Failed { error, .. } => panic!("inspection failed: {error}"),
            }
        }
    }

    #[test]
    fn same_directory_copy_auto_renames_files_and_folders_without_prompting() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = root.path().join("workspace");
        fs::create_dir_all(dir.join("Docs")).expect("create Docs");
        fs::write(dir.join("Docs/a.txt"), b"inside").expect("write Docs/a.txt");
        fs::write(dir.join("report.pdf"), b"pdf").expect("write report.pdf");

        let job = PasteJob::new(
            dir.clone(),
            transfer::Mode::Copy,
            vec![dir.join("report.pdf"), dir.join("Docs")],
        );
        let summary = run_to_completion(job);
        assert!(summary.failures.is_empty());
        assert_eq!(summary.succeeded.len(), 2);
        assert!(!summary.requires_history_reset());
        assert_eq!(
            fs::read(dir.join("report (2).pdf")).expect("copied pdf"),
            b"pdf"
        );
        assert_eq!(
            fs::read(dir.join("Docs (2)/a.txt")).expect("copied folder"),
            b"inside"
        );
    }

    #[test]
    fn file_conflict_supports_replace_rename_skip_and_apply_to_all() {
        let root = tempfile::tempdir().expect("tempdir");
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        fs::create_dir_all(&src).expect("src");
        fs::create_dir_all(&dst).expect("dst");

        for name in ["a.txt", "b.txt", "c.txt"] {
            fs::write(src.join(name), format!("new-{name}")).expect("write src");
            fs::write(dst.join(name), format!("old-{name}")).expect("write dst");
        }

        // 1. Replace a.txt, Rename b.txt -> b-alt.txt, Skip c.txt.
        let mut job = PasteJob::new(
            dst.clone(),
            transfer::Mode::Copy,
            vec![src.join("a.txt"), src.join("b.txt"), src.join("c.txt")],
        );

        let Inspection::Conflict {
            job: mut next,
            conflict,
        } = job.inspect()
        else {
            panic!("expected conflict for a.txt");
        };
        assert_eq!(conflict.kind, ConflictKind::FileIntoFile);
        assert!(conflict.has_more_conflicts_possible);
        next.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Replace,
                apply_to_all: false,
            },
        );
        job = next;

        let Inspection::Conflict {
            job: mut next,
            conflict,
        } = job.inspect()
        else {
            panic!("expected conflict for b.txt");
        };
        next.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Rename(OsString::from("b-alt.txt")),
                apply_to_all: false,
            },
        );
        job = next;

        let Inspection::Conflict {
            job: mut next,
            conflict,
        } = job.inspect()
        else {
            panic!("expected conflict for c.txt");
        };
        assert!(!conflict.has_more_conflicts_possible);
        next.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Skip,
                apply_to_all: false,
            },
        );

        let summary = run_to_completion(next);
        assert!(summary.failures.is_empty());
        assert_eq!(summary.succeeded.len(), 2);
        assert_eq!(summary.skipped, 1);
        assert!(summary.requires_history_reset());
        assert_eq!(
            fs::read_to_string(dst.join("a.txt")).expect("read a.txt"),
            "new-a.txt"
        );
        assert_eq!(
            fs::read_to_string(dst.join("b.txt")).expect("read b.txt"),
            "old-b.txt"
        );
        assert_eq!(
            fs::read_to_string(dst.join("b-alt.txt")).expect("read b-alt.txt"),
            "new-b.txt"
        );
        assert_eq!(
            fs::read_to_string(dst.join("c.txt")).expect("read c.txt"),
            "old-c.txt"
        );

        // 2. Replace all overwrites all remaining files without prompting a second time.
        for name in ["a.txt", "b.txt", "c.txt"] {
            fs::write(dst.join(name), format!("reset-{name}")).expect("reset dst");
        }
        let job2 = PasteJob::new(
            dst.clone(),
            transfer::Mode::Copy,
            vec![src.join("a.txt"), src.join("b.txt"), src.join("c.txt")],
        );
        let Inspection::Conflict {
            job: mut next2,
            conflict,
        } = job2.inspect()
        else {
            panic!("expected first conflict");
        };
        next2.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Replace,
                apply_to_all: true,
            },
        );
        let summary2 = run_to_completion(next2);
        assert_eq!(summary2.succeeded.len(), 3);
        assert_eq!(
            fs::read_to_string(dst.join("c.txt")).expect("read replaced c.txt"),
            "new-c.txt"
        );
    }

    #[test]
    fn folder_merge_expands_recursively_prompts_inner_files_and_cleans_moved_sources() {
        let root = tempfile::tempdir().expect("tempdir");
        let src = root.path().join("src");
        let dst = root.path().join("dst");

        // Source tree: Project/{only_src.txt, shared.txt, sub/{nested_new.txt, nested_hit.txt}}
        fs::create_dir_all(src.join("Project/sub")).expect("src tree");
        fs::write(src.join("Project/only_src.txt"), b"from-src").expect("only_src");
        fs::write(src.join("Project/shared.txt"), b"new-shared").expect("shared");
        fs::write(src.join("Project/sub/nested_new.txt"), b"nested-new").expect("nested_new");
        fs::write(src.join("Project/sub/nested_hit.txt"), b"nested-src").expect("nested_hit");

        // Dest tree: Project/{only_dst.txt, shared.txt, sub/{nested_dst.txt, nested_hit.txt}}
        fs::create_dir_all(dst.join("Project/sub")).expect("dst tree");
        fs::write(dst.join("Project/only_dst.txt"), b"from-dst").expect("only_dst");
        fs::write(dst.join("Project/shared.txt"), b"old-shared").expect("old shared");
        fs::write(dst.join("Project/sub/nested_dst.txt"), b"nested-dst").expect("nested_dst");
        fs::write(dst.join("Project/sub/nested_hit.txt"), b"nested-old").expect("nested_old");

        // Move Project into dst: Merge + apply_to_all should auto-merge `sub/` while STILL
        // prompting for `shared.txt` and `nested_hit.txt`!
        let job = PasteJob::new(dst.clone(), transfer::Mode::Move, vec![src.join("Project")]);
        let Inspection::Conflict { mut job, conflict } = job.inspect() else {
            panic!("expected DirectoryIntoDirectory conflict on Project");
        };
        assert_eq!(conflict.kind, ConflictKind::DirectoryIntoDirectory);
        assert!(conflict.has_more_conflicts_possible);
        job.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Merge,
                apply_to_all: true,
            },
        );

        // First inner file conflict: shared.txt -> Replace (single)
        let Inspection::Conflict { mut job, conflict } = job.inspect() else {
            panic!("expected inner file conflict on shared.txt");
        };
        assert_eq!(conflict.kind, ConflictKind::FileIntoFile);
        assert_eq!(conflict.task.target_name, OsString::from("shared.txt"));
        job.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Replace,
                apply_to_all: false,
            },
        );

        // `sub/` auto-merged because dir_all == Merge; next prompt must be `nested_hit.txt`!
        // Skip `nested_hit.txt` so `src/Project/sub/nested_hit.txt` remains preserved on disk.
        let Inspection::Conflict { mut job, conflict } = job.inspect() else {
            panic!("expected inner file conflict on nested_hit.txt");
        };
        assert_eq!(conflict.kind, ConflictKind::FileIntoFile);
        assert_eq!(conflict.task.target_name, OsString::from("nested_hit.txt"));
        job.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Skip,
                apply_to_all: false,
            },
        );

        let summary = run_to_completion(job);
        assert!(summary.failures.is_empty());
        assert_eq!(summary.merged_dirs, 2);
        assert_eq!(summary.skipped, 1);
        assert!(summary.requires_history_reset());
        // Destination keeps original files + newly moved files + replaced shared.txt.
        assert_eq!(
            fs::read(dst.join("Project/only_dst.txt")).expect("only_dst"),
            b"from-dst"
        );
        assert_eq!(
            fs::read(dst.join("Project/only_src.txt")).expect("only_src"),
            b"from-src"
        );
        assert_eq!(
            fs::read(dst.join("Project/shared.txt")).expect("shared"),
            b"new-shared"
        );
        assert_eq!(
            fs::read(dst.join("Project/sub/nested_new.txt")).expect("nested_new"),
            b"nested-new"
        );
        assert_eq!(
            fs::read(dst.join("Project/sub/nested_dst.txt")).expect("nested_dst"),
            b"nested-dst"
        );
        assert_eq!(
            fs::read(dst.join("Project/sub/nested_hit.txt")).expect("nested_hit"),
            b"nested-old"
        );
        // Because nested_hit.txt was skipped, src/Project/sub must stay intact with that file!
        assert!(src.join("Project/sub/nested_hit.txt").exists());
        assert!(!src.join("Project/only_src.txt").exists());
        assert!(summary.cut_completed.is_empty());

        // Now move a second folder where ALL files succeed -> empty source dir is cleaned up!
        fs::create_dir_all(src.join("Clean/inner")).expect("src clean");
        fs::write(src.join("Clean/inner/f.txt"), b"ok").expect("clean f");
        fs::create_dir_all(dst.join("Clean/inner")).expect("dst clean");
        let job2 = PasteJob::new(dst.clone(), transfer::Mode::Move, vec![src.join("Clean")]);
        let Inspection::Conflict {
            job: mut job2,
            conflict,
        } = job2.inspect()
        else {
            panic!("expected Clean dir conflict");
        };
        job2.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Merge,
                apply_to_all: true,
            },
        );
        let summary2 = run_to_completion(job2);
        assert!(summary2.failures.is_empty());
        assert!(!src.join("Clean").exists());
        assert_eq!(summary2.cut_completed, vec![src.join("Clean")]);
        assert_eq!(
            fs::read(dst.join("Clean/inner/f.txt")).expect("moved nested file"),
            b"ok"
        );
    }

    #[test]
    fn type_mismatch_and_symlink_to_dir_never_merge_or_replace() {
        let root = tempfile::tempdir().expect("tempdir");
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        let real_target = root.path().join("real-target");
        fs::create_dir_all(&src).expect("src");
        fs::create_dir_all(&dst).expect("dst");
        fs::create_dir_all(&real_target).expect("real_target");

        // 1. Symlink pointing to a directory vs real directory -> TypeMismatch (never merge!).
        symlink(&real_target, src.join("linkdir")).expect("symlink");
        fs::create_dir_all(dst.join("linkdir")).expect("dst dir");

        // 2. Regular file vs real directory -> TypeMismatch even after Replace-all!
        fs::write(src.join("a.txt"), b"new").expect("src a.txt");
        fs::write(dst.join("a.txt"), b"old").expect("dst a.txt");
        fs::write(src.join("collide"), b"file").expect("src collide");
        fs::create_dir_all(dst.join("collide")).expect("dst collide dir");

        let job = PasteJob::new(
            dst.clone(),
            transfer::Mode::Copy,
            vec![src.join("a.txt"), src.join("linkdir"), src.join("collide")],
        );

        let Inspection::Conflict { mut job, conflict } = job.inspect() else {
            panic!("expected FileIntoFile on a.txt");
        };
        assert_eq!(conflict.kind, ConflictKind::FileIntoFile);
        job.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Replace,
                apply_to_all: true,
            },
        );

        // Even with Replace-all active, symlink-to-dir vs dir MUST still prompt as TypeMismatch!
        let Inspection::Conflict { mut job, conflict } = job.inspect() else {
            panic!("expected TypeMismatch on linkdir");
        };
        assert_eq!(conflict.kind, ConflictKind::TypeMismatch);
        job.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Skip,
                apply_to_all: false,
            },
        );

        // And file vs dir MUST still prompt as TypeMismatch!
        let Inspection::Conflict { mut job, conflict } = job.inspect() else {
            panic!("expected TypeMismatch on collide");
        };
        assert_eq!(conflict.kind, ConflictKind::TypeMismatch);
        job.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Skip,
                apply_to_all: false,
            },
        );

        let summary = run_to_completion(job);
        assert_eq!(summary.succeeded.len(), 1);
        assert_eq!(summary.skipped, 2);
        assert!(dst.join("collide").is_dir());
    }

    #[test]
    fn apply_to_all_keep_both_and_skip_stay_isolated_per_conflict_kind() {
        let root = tempfile::tempdir().expect("tempdir");
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        fs::create_dir_all(src.join("FolderA")).expect("src FolderA");
        fs::create_dir_all(dst.join("FolderA")).expect("dst FolderA");
        fs::write(src.join("FolderA/inside.txt"), b"inside-a").expect("src inside");
        fs::write(src.join("a.txt"), b"new-a").expect("src a");
        fs::write(dst.join("a.txt"), b"old-a").expect("dst a");
        fs::write(src.join("archive.tar.gz"), b"new-tgz").expect("src tgz");
        fs::write(dst.join("archive.tar.gz"), b"old-tgz").expect("dst tgz");
        fs::write(src.join("clash"), b"file-clash").expect("src clash");
        fs::create_dir_all(dst.join("clash")).expect("dst clash dir");

        // 1. Keep-both (AutoRename) + apply_to_all on FileIntoFile renames all remaining files
        //    (including compound .tar.gz) while STILL prompting for FolderA and clash!
        let job = PasteJob::new(
            dst.clone(),
            transfer::Mode::Copy,
            vec![
                src.join("a.txt"),
                src.join("archive.tar.gz"),
                src.join("FolderA"),
                src.join("clash"),
            ],
        );
        let Inspection::Conflict { mut job, conflict } = job.inspect() else {
            panic!("expected FileIntoFile on a.txt");
        };
        assert_eq!(conflict.kind, ConflictKind::FileIntoFile);
        let suggestion = conflict.suggestion.clone();
        job.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Rename(suggestion),
                apply_to_all: true,
            },
        );

        // archive.tar.gz auto-renamed silently; next conflict MUST be FolderA!
        let Inspection::Conflict { mut job, conflict } = job.inspect() else {
            panic!("expected DirectoryIntoDirectory on FolderA despite file keep-both");
        };
        assert_eq!(conflict.kind, ConflictKind::DirectoryIntoDirectory);
        assert_eq!(conflict.task.target_name, OsString::from("FolderA"));
        job.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Skip,
                apply_to_all: true,
            },
        );

        // Folder Skip-all MUST NOT skip TypeMismatch on `clash`!
        let Inspection::Conflict { mut job, conflict } = job.inspect() else {
            panic!("expected TypeMismatch on clash despite file keep-both and dir skip-all");
        };
        assert_eq!(conflict.kind, ConflictKind::TypeMismatch);
        assert_eq!(conflict.task.target_name, OsString::from("clash"));
        job.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Skip,
                apply_to_all: false,
            },
        );

        let summary = run_to_completion(job);
        assert!(summary.failures.is_empty());
        assert_eq!(summary.succeeded.len(), 2);
        assert_eq!(summary.skipped, 2);
        assert_eq!(
            fs::read(dst.join("a (2).txt")).expect("auto-renamed a.txt"),
            b"new-a"
        );
        assert_eq!(
            fs::read(dst.join("archive (2).tar.gz")).expect("auto-renamed archive.tar.gz"),
            b"new-tgz"
        );
        assert!(!dst.join("FolderA/inside.txt").exists());

        // 2. Skip + apply_to_all on FileIntoFile skips remaining files while STILL prompting for
        //    FolderA!
        let job2 = PasteJob::new(
            dst.clone(),
            transfer::Mode::Copy,
            vec![
                src.join("a.txt"),
                src.join("archive.tar.gz"),
                src.join("FolderA"),
            ],
        );
        let Inspection::Conflict {
            job: mut job2,
            conflict,
        } = job2.inspect()
        else {
            panic!("expected FileIntoFile on a.txt");
        };
        job2.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Skip,
                apply_to_all: true,
            },
        );
        let Inspection::Conflict {
            job: mut job2,
            conflict,
        } = job2.inspect()
        else {
            panic!("expected DirectoryIntoDirectory on FolderA despite file skip-all");
        };
        assert_eq!(conflict.kind, ConflictKind::DirectoryIntoDirectory);
        let dir_suggestion = conflict.suggestion.clone();
        job2.resolve_conflict(
            conflict,
            ConflictResolution {
                action: ConflictAction::Rename(dir_suggestion),
                apply_to_all: false,
            },
        );
        let summary2 = run_to_completion(job2);
        assert!(summary2.failures.is_empty());
        assert_eq!(summary2.skipped, 2);
        assert_eq!(
            fs::read(dst.join("FolderA (2)/inside.txt")).expect("renamed FolderA"),
            b"inside-a"
        );
    }
}
