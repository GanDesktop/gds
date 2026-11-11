//! Selection and filter state independent of the picker window.
use super::{Filter, Options};
use crate::entities::file::directory::{Entry, EntryKind, LinkTarget};
use gds::service::desktop::protocol::chooser;
use std::{
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
            match std::fs::metadata(path) {
                Ok(metadata)
                    if (self.options.directory && metadata.is_dir())
                        || (!self.options.directory && metadata.is_file()) => {}
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
            }
            if !self.options.directory
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

pub(crate) struct Session {
    pub options: Arc<Options>,
    pub active_filter: usize,
    pub read_only: bool,
    pub choices: Vec<chooser::Choice>,
    /// A confirmed selection must only be reported once.
    pub completing: bool,
}

impl Session {
    pub(crate) fn new(options: Options) -> Self {
        let choices = options
            .portal
            .as_ref()
            .map(|request| request.choices.clone())
            .unwrap_or_default();
        Self {
            read_only: options.read_only,
            choices,
            options: Arc::new(options),
            active_filter: 0,
            completing: false,
        }
    }

    pub(crate) fn allows_multiple(&self) -> bool {
        self.options.multiple
    }

    pub(crate) fn accepts_entry(&self, entry: &Entry) -> bool {
        if self.options.directory {
            Projection::is_directory(entry)
        } else {
            !Projection::is_directory(entry)
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

    pub(crate) fn projection(&self) -> Projection {
        Projection {
            options: Arc::clone(&self.options),
            active_filter: self.active_filter,
        }
    }

    pub(crate) fn selection_check(&self, accepted_in_table: Vec<PathBuf>) -> SelectionCheck {
        SelectionCheck {
            options: Arc::clone(&self.options),
            active_filter: self.active_filter,
            accepted_in_table,
        }
    }

    pub(crate) fn portal_result(&self, paths: &[PathBuf]) -> chooser::Result {
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

    /// Emit the confirmed picker result to stdout and update the process exit status.
    ///
    /// # Errors
    /// Propagates serialization or stdout write failures from the underlying emitter.
    pub(crate) fn complete(&mut self, paths: &[PathBuf]) -> Result<(), super::model::Error> {
        self.completing = true;
        let written = if self.options.portal.is_some() {
            let result = self.portal_result(paths);
            super::emit_portal_result(&result)
        } else {
            super::emit_selections(paths, self.read_only, self.active_filter)
        };
        match &written {
            Ok(()) => super::mark_confirmed(),
            Err(_) => super::mark_failed(),
        }
        written
    }

    pub(crate) fn cancel(&mut self) -> bool {
        if self.completing {
            return false;
        }
        self.completing = true;
        super::mark_cancelled();
        true
    }

    pub(crate) fn filters(&self) -> &[Filter] {
        &self.options.filters
    }

    pub(crate) fn select_filter(&mut self, index: usize) -> bool {
        if index == self.active_filter || index >= self.filters().len() {
            return false;
        }
        self.active_filter = index;
        true
    }
}
