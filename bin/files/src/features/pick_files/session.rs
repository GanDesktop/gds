//! Selection and filter state independent of the picker window.
use super::{Filter, Options};
use crate::entities::file::directory::{Entry, EntryKind, LinkTarget};
use gds::service::desktop::protocol::chooser;
use std::{path::Path, sync::Arc};

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
            .is_some_and(|filter| filter.label.is_some())
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

    pub(crate) fn filters(&self) -> &[Filter] {
        &self.options.filters
    }

    pub(crate) fn matches(&self, path: &Path) -> bool {
        self.options.matches(self.active_filter, path)
    }

    pub(crate) fn select_filter(&mut self, index: usize) -> bool {
        if index == self.active_filter || index >= self.filters().len() {
            return false;
        }
        self.active_filter = index;
        true
    }
}
