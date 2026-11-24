use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use gds::service::{remote, task_manager::Owner as TaskOwner};

use crate::{
    entities::{
        file::{ViewMode, columns, directory, sorting},
        location::{Location, navigation::Navigation, places::Places, volumes},
    },
    features::{
        file_operations::{history, paste, recycle, transfer},
        pick_files,
    },
    shared::lib::selection::{Selection, SelectionBacking},
};

#[derive(Default)]
pub(super) struct Visit {
    pub selection: Selection<RowKey>,
    pub anchor: Option<RowKey>,
    pub cursor: Option<RowKey>,
    pub scroll: f32,
    pub filter: String,
}

pub(super) enum Contents {
    LoadingCollection,
    Unavailable(String),
    Directory,
    Recent(Vec<directory::Entry>),
    Places(Vec<Location>),
    Network {
        mounts: Vec<PathBuf>,
        error: Option<String>,
    },
    Trash(Vec<recycle::Entry>),
    Tags {
        listing: crate::entities::tags::Listing,
        backend: crate::entities::tags::Baloo,
        entries: Vec<directory::Entry>,
    },
    Remote {
        listing: remote::Listing,
        session: remote::Session,
    },
    Volumes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AddressPresentation {
    Breadcrumbs,
    Editing { original: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum RowKey {
    Local(PathBuf),
    Place(Location),
    Trash(std::ffi::OsString),
    Tag(String),
    Remote(remote::Address),
    Volume(volumes::VolumeId),
}

#[derive(Clone)]
pub(super) enum Row {
    File(directory::Entry),
    Place(Location),
    Trash(recycle::Entry),
    Tag(String),
    Remote(remote::Entry),
    Volume(volumes::Volume),
}

impl Row {
    pub fn key(&self) -> RowKey {
        match self {
            Self::File(entry) => RowKey::Local(entry.path.clone()),
            Self::Place(location) => RowKey::Place(location.clone()),
            Self::Trash(entry) => RowKey::Trash(entry.id.clone()),
            Self::Tag(tag) => RowKey::Tag(tag.clone()),
            Self::Remote(entry) => RowKey::Remote(entry.address.clone()),
            Self::Volume(volume) => RowKey::Volume(volume.id.clone()),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::File(entry) => Some(&entry.path),
            Self::Trash(entry) => Some(&entry.original_path),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::File(entry) => entry.display_name.clone(),
            Self::Place(location) => location.label(),
            Self::Trash(entry) => entry.name.to_string_lossy().into_owned(),
            Self::Tag(tag) => tag.rsplit('/').next().unwrap_or(tag).to_owned(),
            Self::Remote(entry) => entry.name.clone(),
            Self::Volume(volume) => volume.name.clone(),
        }
    }

    pub fn matches_prefix(&self, lowercase_prefix: &str) -> bool {
        if lowercase_prefix.is_empty() {
            return false;
        }
        match self {
            Self::File(entry) => entry.matches_prefix(lowercase_prefix),
            Self::Place(location) => str_starts_with_lowercase(&location.label(), lowercase_prefix),
            Self::Trash(entry) => {
                str_starts_with_lowercase(&entry.name.to_string_lossy(), lowercase_prefix)
            }
            Self::Tag(tag) => {
                let leaf = tag.rsplit('/').next().unwrap_or(tag);
                str_starts_with_lowercase(leaf, lowercase_prefix)
            }
            Self::Remote(entry) => str_starts_with_lowercase(&entry.name, lowercase_prefix),
            Self::Volume(volume) => str_starts_with_lowercase(&volume.name, lowercase_prefix),
        }
    }

    pub fn icon(&self) -> gds::ui::IconName {
        use gds::ui::IconName;
        match self {
            Self::File(entry) => {
                if entry.kind == directory::EntryKind::Directory {
                    IconName::Folder
                } else {
                    IconName::File
                }
            }
            Self::Place(location) => location.icon(),
            Self::Trash(entry) => match entry.kind {
                directory::EntryKind::Directory => IconName::Folder,
                directory::EntryKind::SymbolicLink => IconName::Link,
                directory::EntryKind::File => IconName::File,
                _ => IconName::FileQuestionMark,
            },
            Self::Tag(_) => IconName::Tag,
            Self::Remote(entry) => {
                if entry.kind == remote::EntryKind::Directory {
                    IconName::Folder
                } else {
                    IconName::File
                }
            }
            Self::Volume(_) => IconName::HardDrive,
        }
    }
}

fn str_starts_with_lowercase(haystack: &str, lowercase_prefix: &str) -> bool {
    if haystack.is_ascii() {
        lowercase_prefix.is_ascii()
            && haystack
                .as_bytes()
                .get(..lowercase_prefix.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(lowercase_prefix.as_bytes()))
    } else {
        haystack.to_lowercase().starts_with(lowercase_prefix)
    }
}

#[derive(Clone, Copy)]
pub(super) enum InitialVisit {
    Restored,
    Explicit,
}

pub(super) struct Tab {
    pub id: u64,
    pub navigation: Navigation<Visit>,
    pub contents: Contents,
    pub rows: Vec<Row>,
    pub backing: Arc<SelectionBacking<RowKey>>,
    pub mode: ViewMode,
    pub sort: sorting::Sort,
    pub columns: columns::Layout,
    pub pinned: bool,
    pub initial: InitialVisit,
    pub work: Option<TaskOwner>,
    pub enrichment: Option<super::enrichment::Enrichment>,
    pub reveal: Vec<PathBuf>,
    pub reveal_hidden: bool,
}

impl Tab {
    pub fn new(id: u64, location: Location, places: &Places, pinned: bool) -> Self {
        let path = location.local_path().map(Path::to_owned);
        Self {
            id,
            mode: places.effective_view_mode(location.clone()),
            sort: places.effective_sort(location.clone()),
            columns: places.effective_columns(location.clone()).clone(),
            navigation: Navigation::new(location, path),
            contents: Contents::Directory,
            rows: Vec::new(),
            backing: SelectionBacking::from_items(std::iter::empty()),
            pinned,
            initial: InitialVisit::Explicit,
            work: None,
            enrichment: None,
            reveal: Vec::new(),
            reveal_hidden: false,
        }
    }

    pub fn restored(mut self) -> Self {
        self.initial = InitialVisit::Restored;
        self
    }

    pub fn cancel(&mut self) {
        self.enrichment = None;
        if let Some(scope) = self.work.take() {
            scope.cancel();
        }
    }

    pub fn selected(&self) -> Vec<Row> {
        self.rows
            .iter()
            .filter(|row| self.navigation.view().selection.contains(&row.key()))
            .cloned()
            .collect()
    }

    pub fn is_pure_tag_catalogue(&self) -> bool {
        matches!(self.contents, Contents::Tags { .. })
            && !self.rows.iter().any(|row| matches!(row, Row::File(_)))
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(super) const FILES_TASK_KEY: &str = "files";
pub(super) const TRASH_TASK_KEY: &str = "trash";

pub(super) fn remote_task_key(id: u64) -> String {
    format!("remote-task-{id}")
}

/// Concurrent window-owned background operation entry shown in the footer `ListTodo` popover.
#[derive(Clone)]
pub(super) struct BackgroundTask {
    pub key: String,
    pub label: String,
    pub detail: Option<String>,
    #[cfg(test)]
    pub progress: Option<f32>,
    pub cancel: tokio_util::sync::CancellationToken,
    on_cancel: Option<Rc<dyn Fn()>>,
}

impl BackgroundTask {
    pub fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            detail: None,
            #[cfg(test)]
            progress: None,
            cancel,
            on_cancel: None,
        }
    }

    #[cfg(test)]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        self.detail = (!detail.is_empty()).then_some(detail);
        self
    }

    #[cfg(test)]
    pub fn with_progress(mut self, progress: f32) -> Self {
        self.progress = progress.is_finite().then(|| progress.clamp(0.0, 1.0));
        self
    }

    #[cfg(test)]
    pub fn with_on_cancel(mut self, on_cancel: impl Fn() + 'static) -> Self {
        self.on_cancel = Some(Rc::new(on_cancel));
        self
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn cancel(&self) {
        let was_cancelled = self.cancel.is_cancelled();
        self.cancel.cancel();
        if !was_cancelled && let Some(callback) = &self.on_cancel {
            callback();
        }
    }

    pub fn display_label(&self) -> String {
        match self.detail.as_deref().filter(|detail| !detail.is_empty()) {
            Some(detail) => format!("{} — {detail}", self.label),
            None => self.label.clone(),
        }
    }
}

/// Ordered map of concurrent background tasks (`"files"`, `"trash"`, `"remote-task-{id}"`).
#[derive(Clone, Default)]
pub(super) struct BackgroundTasks {
    items: Vec<BackgroundTask>,
}

impl BackgroundTasks {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn items(&self) -> &[BackgroundTask] {
        &self.items
    }

    pub fn get(&self, key: &str) -> Option<&BackgroundTask> {
        self.items.iter().find(|item| item.key == key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.items.iter().any(|item| item.key == key)
    }

    pub fn is_busy(&self, key: &str) -> bool {
        self.contains(key)
    }

    pub fn begin(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> bool {
        let key = key.into();
        if self.contains(&key) {
            return false;
        }
        self.items.push(BackgroundTask::new(key, label, cancel));
        true
    }

    #[cfg(test)]
    pub fn upsert(&mut self, task: BackgroundTask) {
        if let Some(existing) = self.items.iter_mut().find(|item| item.key == task.key) {
            *existing = task;
        } else {
            self.items.push(task);
        }
    }

    pub fn set_detail(&mut self, key: &str, detail: Option<String>) -> bool {
        let normalized = detail.filter(|text| !text.is_empty());
        let Some(item) = self.items.iter_mut().find(|item| item.key == key) else {
            return false;
        };
        if item.detail == normalized {
            return false;
        }
        item.detail = normalized;
        true
    }

    #[cfg(test)]
    pub fn update_progress(
        &mut self,
        key: &str,
        label: Option<String>,
        detail: Option<String>,
        progress: Option<f32>,
    ) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.key == key) else {
            return false;
        };
        if let Some(label) = label {
            item.label = label;
        }
        item.detail = detail.filter(|text| !text.is_empty());
        item.progress = progress
            .filter(|value| value.is_finite())
            .map(|value| value.clamp(0.0, 1.0));
        true
    }

    pub fn sync(
        &mut self,
        key: impl Into<String>,
        task: Option<(String, tokio_util::sync::CancellationToken)>,
    ) {
        let key = key.into();
        if let Some((label, cancel)) = task {
            if let Some(existing) = self.items.iter_mut().find(|item| item.key == key) {
                existing.label = label;
                existing.cancel = cancel;
            } else {
                self.items.push(BackgroundTask::new(key, label, cancel));
            }
        } else {
            self.items.retain(|item| item.key != key);
        }
    }

    pub fn finish(&mut self, key: &str) -> Option<BackgroundTask> {
        let index = self.items.iter().position(|item| item.key == key)?;
        Some(self.items.remove(index))
    }

    pub fn cancel(&self, key: &str) -> bool {
        let Some(item) = self.get(key) else {
            return false;
        };
        item.cancel();
        true
    }

    pub fn cancel_all(&self) {
        for item in &self.items {
            item.cancel();
        }
    }
}

pub(super) enum Operation {
    Idle,
    Running {
        label: String,
        cancel: tokio_util::sync::CancellationToken,
    },
}

impl Operation {
    pub fn busy(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    pub fn cancel(&self) {
        if let Self::Running { cancel, .. } = self {
            cancel.cancel();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pane {
    Files,
    Settings,
    Help,
}

#[derive(Clone, Default)]
pub(super) struct Pages {
    stack: Vec<Pane>,
}

impl Pages {
    pub fn current(&self) -> Pane {
        self.stack.last().copied().unwrap_or(Pane::Files)
    }

    pub fn open(&mut self, page: Pane) {
        if page == Pane::Files {
            self.stack.clear();
        } else if let Some(index) = self.stack.iter().position(|current| *current == page) {
            self.stack.truncate(index + 1);
        } else {
            self.stack.push(page);
        }
    }

    pub fn back(&mut self) {
        self.stack.pop();
    }
}

/// Optional visual metadata and frozen item preview for [`Prompt::Confirm`].
#[derive(Clone, Default)]
pub(super) struct ConfirmOptions {
    pub icon: Option<gds::ui::IconName>,
    pub primary_icon: Option<gds::ui::IconName>,
    pub destructive: bool,
    pub items: Vec<String>,
    pub overflow_count: usize,
}

#[derive(Clone)]
pub(super) enum Prompt {
    Closed,
    About,
    Name {
        form: super::name_form::NameForm,
        submit: Rc<dyn Fn(std::ffi::OsString)>,
    },
    Conflict {
        mode: transfer::Mode,
        conflict: paste::PasteConflict,
        resolve: Rc<dyn Fn(paste::ConflictResolution)>,
        cancel: Option<Rc<dyn Fn()>>,
    },
    Confirm {
        title: String,
        message: String,
        primary: String,
        accept: Rc<dyn Fn()>,
    },
}

pub(super) struct State {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub next_tab: u64,
    pub home: PathBuf,
    pub places: Places,
    pub history: history::History,
    pub picker: Option<pick_files::Session>,
    pub operation: Operation,
    pub saving_places: bool,
    pub favorites_reload: Option<Vec<Location>>,
    pub pending_device_removals: Vec<volumes::DeviceAction>,
}

impl State {
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    pub fn tab_by_id(&mut self, id: u64) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn allows_multiple(&self) -> bool {
        matches!(
            self.tab().contents,
            Contents::Directory | Contents::Trash(_) | Contents::Remote { .. }
        ) && self
            .picker
            .as_ref()
            .is_none_or(|picker| picker.allows_multiple())
    }
}

#[cfg(test)]
mod tests;
