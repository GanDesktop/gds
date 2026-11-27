//! Selection and filter state independent of the picker window.
use super::{Filter, Options, Outcome};
use crate::entities::file::directory::{Entry, EntryKind, LinkTarget};
use gds::service::desktop::protocol::chooser;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValidationFailure {
    pub message_id: &'static str,
    pub detail: Option<String>,
}

impl ValidationFailure {
    pub(crate) const fn new(message_id: &'static str) -> Self {
        Self {
            message_id,
            detail: None,
        }
    }

    pub(crate) fn with_detail(message_id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            message_id,
            detail: Some(detail.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivatedPath {
    Directory,
    File,
}

/// Inspect an activated picker row path off the UI thread.
///
/// # Errors
/// Returns [`ValidationFailure`] when the path is missing or is neither a directory nor a regular file.
pub(crate) fn inspect_activated_path(path: &Path) -> Result<ActivatedPath, ValidationFailure> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(ActivatedPath::Directory),
        Ok(metadata) if metadata.is_file() => Ok(ActivatedPath::File),
        Ok(_) => Err(ValidationFailure::new("picker-not-file")),
        Err(error) => Err(ValidationFailure::with_detail(
            "picker-missing",
            error.to_string(),
        )),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SaveDecision {
    Ready(Vec<PathBuf>),
    ConfirmOverwrite(Vec<PathBuf>),
}

/// Resolve and inspect save targets off the UI thread.
///
/// # Errors
/// Returns [`ValidationFailure`] when save targets cannot be allocated or point to a directory.
pub(crate) fn prepare_save_targets(
    mode: &chooser::Mode,
    mut paths: Vec<PathBuf>,
) -> Result<SaveDecision, ValidationFailure> {
    if let chooser::Mode::SaveFiles { names } = mode {
        let [folder] = paths.as_slice() else {
            return Err(ValidationFailure::new("picker-select-directory"));
        };
        paths = super::targets::unique(folder, names)
            .map_err(|error| ValidationFailure::with_detail("picker-missing", error.to_string()))?;
        return Ok(SaveDecision::Ready(paths));
    }
    if paths.iter().any(|path| path.is_dir()) {
        return Err(ValidationFailure::new("picker-not-file"));
    }
    let occupied = paths
        .iter()
        .try_fold(false, |occupied, path| {
            super::targets::occupied(path).map(|current| occupied || current)
        })
        .map_err(|error| ValidationFailure::with_detail("picker-missing", error.to_string()))?;
    if occupied {
        Ok(SaveDecision::ConfirmOverwrite(paths))
    } else {
        Ok(SaveDecision::Ready(paths))
    }
}

#[derive(Clone)]
pub(crate) struct SelectionCheck {
    options: Arc<Options>,
    active_filter: usize,
    accepted_in_table: Vec<PathBuf>,
}

impl SelectionCheck {
    /// Validate candidate selection paths against filesystem metadata and the active filter group.
    ///
    /// # Errors
    /// Returns [`ValidationFailure`] when the selection count, entry kind, or filter match is invalid.
    pub(crate) fn validate(&self, paths: &[PathBuf]) -> Result<(), ValidationFailure> {
        let save_files_mode = matches!(
            self.options.portal.as_ref().map(|request| &request.mode),
            Some(chooser::Mode::SaveFiles { .. })
        );
        if paths.is_empty() || (!self.options.multiple && paths.len() != 1 && !save_files_mode) {
            return Err(ValidationFailure::new(if self.options.directory {
                "picker-select-directory"
            } else {
                "picker-select-file"
            }));
        }
        let saving = self.options.is_save();
        for path in paths {
            if saving {
                if path.is_dir() || !path.parent().is_some_and(Path::is_dir) {
                    return Err(ValidationFailure::new("picker-not-file"));
                }
                continue;
            }
            let is_directory = match std::fs::metadata(path) {
                Ok(metadata)
                    if ((self.options.directory || self.options.allow_mixed)
                        && metadata.is_dir())
                        || (!self.options.directory && metadata.is_file()) =>
                {
                    metadata.is_dir()
                }
                Ok(_) => {
                    return Err(ValidationFailure::new(if self.options.directory {
                        "picker-not-directory"
                    } else {
                        "picker-not-file"
                    }));
                }
                Err(error) => {
                    return Err(ValidationFailure::with_detail(
                        "picker-missing",
                        error.to_string(),
                    ));
                }
            };
            if !is_directory
                && !self.accepted_in_table.contains(path)
                && !self.options.matches(self.active_filter, path)
            {
                return Err(ValidationFailure::new("picker-filter-mismatch"));
            }
        }
        Ok(())
    }
}

/// The visible file projection of one filter group.
///
/// Every tab keeps a copy so a directory read projects its fresh entries
/// through the same filter instead of replacing a narrowed list with the whole
/// folder.
#[derive(Clone)]
pub(crate) struct Projection {
    options: Arc<Options>,
    active_filter: usize,
}

impl Projection {
    /// Whether an entry belongs in the list: every folder stays visible and
    /// navigable, files have to match the active group.
    pub(crate) fn keeps(&self, entry: &Entry) -> bool {
        Self::is_directory(entry)
            || (!self.options.directory
                && self.options.matches_type(
                    self.active_filter,
                    &entry.path,
                    entry
                        .details
                        .display_content()
                        .and_then(|content| content.mime.as_deref()),
                ))
    }

    /// Whether the active group narrows files at all.
    pub(crate) fn narrows(&self) -> bool {
        self.options
            .filters
            .get(self.active_filter)
            .is_some_and(Filter::narrows)
    }

    /// Whether an entry is a folder, following a link only for navigation.
    pub(crate) fn is_directory(entry: &Entry) -> bool {
        match entry.kind {
            EntryKind::Directory => true,
            EntryKind::SymbolicLink => entry.details.target.as_ref().is_some_and(|target| {
                matches!(
                    target,
                    LinkTarget::Resolved {
                        kind: EntryKind::Directory,
                        ..
                    }
                )
            }),
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ChoiceControl {
    pub id: String,
    pub label: String,
    pub options: Vec<(String, String)>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ActivatedFile {
    PopulateSaveName(Option<String>),
    Confirm,
}

#[derive(Debug)]
pub(crate) enum AcceptPlan {
    Save {
        mode: chooser::Mode,
        target: PathBuf,
    },
    SaveFiles {
        mode: chooser::Mode,
        folder: PathBuf,
    },
    Open {
        paths: Vec<PathBuf>,
    },
}

pub(crate) struct Session {
    options: Arc<Options>,
    active_filter: usize,
    read_only: bool,
    choices: Vec<chooser::Choice>,
    /// Lossless spelling retained while the filename input still displays this name.
    save_name: Option<OsString>,
    submitting: bool,
    /// A confirmed selection must only be reported once.
    completing: bool,
    outcome: Option<Outcome>,
}

impl Session {
    pub(crate) fn new(options: Options) -> Self {
        let choices = options
            .portal
            .as_ref()
            .map(|request| request.choices.clone())
            .unwrap_or_default();
        let save_name = options
            .portal
            .as_ref()
            .and_then(|request| match &request.mode {
                chooser::Mode::Save { name } => Some(
                    request
                        .current_file
                        .as_ref()
                        .and_then(|file| Path::new(file).file_name())
                        .unwrap_or(name)
                        .to_owned(),
                ),
                _ => None,
            });
        Self {
            save_name,
            read_only: options.read_only,
            choices,
            options: Arc::new(options),
            active_filter: 0,
            submitting: false,
            completing: false,
            outcome: None,
        }
    }

    pub(crate) fn allows_multiple(&self) -> bool {
        self.options.multiple
    }

    pub(crate) fn is_save(&self) -> bool {
        self.options.is_save()
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.submitting || self.completing
    }

    pub(crate) fn supports_read_only(&self) -> bool {
        !self.options.directory && !self.options.is_save()
    }

    pub(crate) fn accept_label_parts(&self) -> (Option<&str>, &'static str) {
        let custom = self
            .options
            .portal
            .as_ref()
            .and_then(|portal| portal.accept_label.as_deref());
        let fallback_id = if self.options.is_save() {
            "picker-save"
        } else if self.options.directory {
            "picker-confirm-directory"
        } else {
            "picker-confirm"
        };
        (custom, fallback_id)
    }

    pub(crate) fn initial_save_name(&self) -> Option<String> {
        self.save_name
            .as_ref()
            .map(|name| name.to_string_lossy().into_owned())
    }

    /// Resolve double-clicking a regular file in the picker list.
    ///
    /// Save mode populates the filename input and never submits; open mode confirms selection.
    pub(crate) fn handle_activated_file(&mut self, path: &Path) -> ActivatedFile {
        if !self.options.is_save() {
            return ActivatedFile::Confirm;
        }
        let display = path.file_name().map(|name| {
            self.save_name = Some(name.to_os_string());
            name.to_string_lossy().into_owned()
        });
        ActivatedFile::PopulateSaveName(display)
    }

    pub(crate) fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) fn set_read_only(&mut self, read_only: bool) -> bool {
        if !self.supports_read_only() || self.read_only == read_only {
            return false;
        }
        self.read_only = read_only;
        true
    }

    pub(crate) fn choice_controls(&self) -> Vec<ChoiceControl> {
        self.choices
            .iter()
            .map(|choice| ChoiceControl {
                id: choice.id.clone(),
                label: choice.label.clone(),
                options: choice.options.clone(),
            })
            .collect()
    }

    pub(crate) fn choice_selected(&self, id: &str) -> Option<&str> {
        self.choices
            .iter()
            .find(|choice| choice.id == id)
            .map(|choice| choice.selected.as_str())
    }

    pub(crate) fn set_choice(&mut self, id: &str, value: &str) -> bool {
        let Some(choice) = self.choices.iter_mut().find(|choice| choice.id == id) else {
            return false;
        };
        if choice.selected == value {
            return false;
        }
        choice.selected = value.to_owned();
        true
    }

    pub(crate) fn accepts_entry(&self, entry: &Entry) -> bool {
        if Projection::is_directory(entry) {
            self.options.directory || self.options.allow_mixed
        } else {
            !self.options.directory
                && self.options.matches_type(
                    self.active_filter,
                    &entry.path,
                    entry
                        .details
                        .display_content()
                        .and_then(|content| content.mime.as_deref()),
                )
        }
    }

    pub(crate) fn resolve_candidates(
        &self,
        typed_save: &str,
        current_dir: Option<&Path>,
        selected: &[Option<&Entry>],
    ) -> Option<Vec<PathBuf>> {
        if self.is_busy() {
            return None;
        }

        if self.options.is_save() {
            let dir = current_dir?;
            let file_name = if let Some(original) = self.save_name.as_ref()
                && original.to_string_lossy() == typed_save
            {
                original.clone()
            } else {
                OsString::from(typed_save)
            };
            if !Options::valid_name(&file_name) {
                return None;
            }
            return Some(vec![dir.join(file_name)]);
        }

        if matches!(
            self.options.portal.as_ref().map(|portal| &portal.mode),
            Some(chooser::Mode::SaveFiles { .. })
        ) {
            let folder = selected
                .iter()
                .find_map(|entry| {
                    entry
                        .filter(|entry| Projection::is_directory(entry))
                        .map(|entry| entry.path.clone())
                })
                .or_else(|| current_dir.map(Path::to_path_buf))?;
            return Some(vec![folder]);
        }

        if self.options.directory {
            if selected.is_empty() {
                return current_dir.map(|dir| vec![dir.to_path_buf()]);
            }
            if !self.allows_multiple() && selected.len() > 1 {
                return None;
            }
            let mut dirs = Vec::with_capacity(selected.len());
            for entry in selected {
                match entry {
                    Some(entry) if Projection::is_directory(entry) => {
                        dirs.push(entry.path.clone());
                    }
                    _ => return None,
                }
            }
            return (!dirs.is_empty()).then_some(dirs);
        }

        if selected.is_empty() || (!self.allows_multiple() && selected.len() > 1) {
            return None;
        }
        let mut files = Vec::with_capacity(selected.len());
        for entry in selected {
            match entry {
                Some(entry) if self.accepts_entry(entry) => {
                    files.push(entry.path.clone());
                }
                _ => return None,
            }
        }
        (!files.is_empty()).then_some(files)
    }

    pub(crate) fn begin_accept(&mut self, candidates: Vec<PathBuf>) -> Option<AcceptPlan> {
        if self.is_busy() {
            return None;
        }
        if self.options.is_save() {
            let target = candidates.into_iter().next()?;
            let mode = self
                .options
                .portal
                .as_ref()
                .map(|portal| portal.mode.clone())
                .unwrap_or(chooser::Mode::Save {
                    name: OsString::new(),
                });
            self.submitting = true;
            Some(AcceptPlan::Save { mode, target })
        } else if let Some(chooser::Mode::SaveFiles { names }) = self
            .options
            .portal
            .as_ref()
            .map(|portal| &portal.mode)
            .cloned()
        {
            let folder = candidates.into_iter().next()?;
            self.submitting = true;
            Some(AcceptPlan::SaveFiles {
                mode: chooser::Mode::SaveFiles { names },
                folder,
            })
        } else {
            self.submitting = true;
            Some(AcceptPlan::Open { paths: candidates })
        }
    }

    pub(crate) fn begin_validation(&mut self, accepted_in_table: Vec<PathBuf>) -> SelectionCheck {
        self.submitting = true;
        self.selection_check(accepted_in_table)
    }

    pub(crate) fn finish_submit(&mut self) {
        self.submitting = false;
    }

    pub(crate) fn mark_completing(&mut self) {
        self.completing = true;
    }

    pub(crate) fn projection(&self) -> Projection {
        Projection {
            options: Arc::clone(&self.options),
            active_filter: self.active_filter,
        }
    }

    pub(super) fn selection_check(&self, accepted_in_table: Vec<PathBuf>) -> SelectionCheck {
        SelectionCheck {
            options: Arc::clone(&self.options),
            active_filter: self.active_filter,
            accepted_in_table,
        }
    }

    pub(super) fn portal_result(&self, paths: &[PathBuf]) -> chooser::Result {
        chooser::Result {
            paths: paths
                .iter()
                .map(|path| path.clone().into_os_string())
                .collect(),
            filter_index: self.active_filter,
            read_only: self.read_only,
            choices: self
                .choices
                .iter()
                .map(|choice| (choice.id.clone(), choice.selected.clone()))
                .collect(),
        }
    }

    fn record_outcome(&mut self, outcome: Outcome) -> bool {
        if self.outcome.is_some() {
            return false;
        }
        self.completing = true;
        self.outcome = Some(outcome);
        if let Some(slot) = &self.options.outcome {
            slot.record(outcome);
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn outcome(&self) -> Outcome {
        self.outcome.unwrap_or_default()
    }

    /// Emit the confirmed picker result to stdout and record the session outcome once.
    ///
    /// Returns `None` if completion has already started.
    pub(crate) fn complete(
        &mut self,
        paths: &[PathBuf],
    ) -> Option<Result<(), super::model::Error>> {
        if self.completing {
            return None;
        }
        let stdout = std::io::stdout().lock();
        Some(self.complete_to(stdout, paths))
    }

    pub(super) fn complete_to(
        &mut self,
        writer: impl std::io::Write,
        paths: &[PathBuf],
    ) -> Result<(), super::model::Error> {
        self.completing = true;
        let written = if self.options.portal.is_some() {
            let result = self.portal_result(paths);
            super::model::write_portal_result(writer, &result)
        } else {
            super::model::write_selections(writer, paths, self.read_only, self.active_filter)
        };
        let outcome = match &written {
            Ok(()) => Outcome::Confirmed,
            Err(_) => Outcome::Failed,
        };
        self.record_outcome(outcome);
        written
    }

    pub(crate) fn cancel(&mut self) -> bool {
        if self.completing {
            return false;
        }
        self.record_outcome(Outcome::Cancelled)
    }

    pub(crate) fn filters(&self) -> &[Filter] {
        if self.options.directory {
            &[]
        } else {
            &self.options.filters
        }
    }

    pub(crate) fn active_filter(&self) -> usize {
        self.active_filter
    }

    pub(crate) fn next_filter_index(&self) -> Option<usize> {
        let count = self.filters().len();
        (count > 1).then_some((self.active_filter + 1) % count)
    }

    pub(crate) fn select_filter(&mut self, index: usize) -> bool {
        if index == self.active_filter || index >= self.filters().len() {
            return false;
        }
        self.active_filter = index;
        true
    }
}
