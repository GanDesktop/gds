use crate::{
    entities::{
        file::{ViewMode, columns, directory, sorting},
        location::{Location, navigation::Navigation, places::Places, volumes},
    },
    features::{
        file_operations::{history, recycle},
        pick_files,
    },
    shared::lib::selection::Selection,
};
use gds::service::{remote, task_manager::Owner as TaskOwner};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
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

#[derive(Clone, Debug, PartialEq)]
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
    pub fn icon(&self) -> gds::ui::Icon {
        use gds::ui::Icon;
        match self {
            Self::File(entry) => {
                if entry.kind == directory::EntryKind::Directory {
                    Icon::Folder
                } else {
                    Icon::File
                }
            }
            Self::Place(location) => location.icon(),
            Self::Trash(entry) => match entry.kind {
                directory::EntryKind::Directory => Icon::Folder,
                directory::EntryKind::SymbolicLink => Icon::Link,
                directory::EntryKind::File => Icon::File,
                _ => Icon::Unknown,
            },
            Self::Tag(_) => Icon::Tag,
            Self::Remote(entry) => {
                if entry.kind == remote::EntryKind::Directory {
                    Icon::Folder
                } else {
                    Icon::File
                }
            }
            Self::Volume(_) => Icon::Drive,
        }
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
    pub backing: Arc<crate::shared::lib::selection::SelectionBacking<RowKey>>,
    pub mode: ViewMode,
    pub sort: sorting::Sort,
    pub columns: columns::Layout,
    pub pinned: bool,
    pub initial: InitialVisit,
    pub work: Option<TaskOwner>,
    pub enrichment: Option<super::enrichment::Enrichment>,
    pub reveal: Vec<PathBuf>,
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
            backing: crate::shared::lib::selection::SelectionBacking::from_items(std::iter::empty()),
            pinned,
            initial: InitialVisit::Explicit,
            work: None,
            enrichment: None,
            reveal: Vec::new(),
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
}
impl Drop for Tab {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Clone)]
pub(super) struct Origin {
    pub tab: u64,
    pub location: Location,
}

pub(super) enum Operation {
    Idle,
    Running {
        label: String,
        cancel: tokio_util::sync::CancellationToken,
        origin: Origin,
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

#[derive(Clone)]
pub(super) enum Prompt {
    Closed,
    About,
    Name {
        form: super::name_form::NameForm,
        submit: Rc<dyn Fn(std::ffi::OsString)>,
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
