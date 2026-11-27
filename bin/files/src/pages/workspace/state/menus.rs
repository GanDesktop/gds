//! Pure menu surface, target snapshot, and column-scope state.

use std::path::PathBuf;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use super::{
    model::{Row, RowKey, State},
    ui_flags::StatusKind,
};
use crate::{
    entities::{
        file::columns::Layout,
        location::{Location, volumes},
    },
    shared::lib::selection::Selection,
};

#[derive(Clone, PartialEq, Eq)]
pub(in crate::pages::workspace) struct DirectoryTarget {
    tab: u64,
    path: PathBuf,
}

impl DirectoryTarget {
    pub(in crate::pages::workspace) fn from_state(state: &State) -> Option<Self> {
        let tab = state.tab();
        tab.navigation
            .path()
            .filter(|_| !tab.navigation.is_loading())
            .map(|path| Self {
                tab: tab.id,
                path: path.to_owned(),
            })
    }

    pub(in crate::pages::workspace) fn matches_state(&self, state: &State) -> bool {
        state.tab().id == self.tab && state.tab().navigation.path() == Some(self.path.as_path())
    }

    pub(in crate::pages::workspace) fn removal_in_progress(
        &self,
        volumes: &volumes::Service,
    ) -> bool {
        volumes.removal_in_progress(&self.path)
    }

    pub(in crate::pages::workspace) fn current_path(&self, state: &State) -> Option<PathBuf> {
        self.matches_state(state).then(|| self.path.clone())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::pages::workspace) struct SelectionTarget {
    tab: u64,
    location: Location,
    row: RowKey,
    selected: Selection<RowKey>,
}

impl SelectionTarget {
    pub(in crate::pages::workspace) fn from_state(state: &State, row: RowKey) -> Self {
        Self {
            tab: state.tab().id,
            location: state.tab().navigation.location().clone(),
            row,
            selected: state.tab().navigation.view().selection.clone(),
        }
    }

    pub(in crate::pages::workspace) fn matches_state(&self, state: &State) -> bool {
        let tab = state.tab();
        tab.id == self.tab
            && *tab.navigation.location() == self.location
            && tab.rows.iter().any(|row| row.key() == self.row)
            && tab.navigation.view().selection == self.selected
    }

    pub(in crate::pages::workspace) fn resolve_row(&self, state: &State) -> Option<Row> {
        state
            .tab()
            .rows
            .iter()
            .find(|row| row.key() == self.row)
            .cloned()
    }
}

#[derive(Clone, Copy)]
pub(in crate::pages::workspace) enum DirectoryAction {
    NewFolder,
    Paste,
    InternalTerminal,
    Terminal,
    Refresh,
}

impl DirectoryAction {
    pub(in crate::pages::workspace) fn label(self) -> &'static str {
        match self {
            Self::NewFolder => "new-folder",
            Self::Paste => "paste-files",
            Self::InternalTerminal => "files-terminal-open-here",
            Self::Terminal => "terminal-open-external",
            Self::Refresh => "refresh",
        }
    }
}

#[derive(Clone)]
pub(in crate::pages::workspace) enum Surface {
    Closed,
    Application,
    Page,
    Sort,
    Selection {
        target: SelectionTarget,
        x: f32,
        y: f32,
    },
    Directory {
        target: DirectoryTarget,
        x: f32,
        y: f32,
    },
    Trash {
        tab: u64,
        x: f32,
        y: f32,
    },
    Tab {
        id: u64,
        x: f32,
        y: f32,
    },
    Volume {
        volume: volumes::Volume,
        x: f32,
        y: f32,
    },
    QuickAccessSection {
        x: f32,
        y: f32,
    },
    QuickAccessItem {
        path: PathBuf,
        x: f32,
        y: f32,
    },
    History {
        tab: u64,
        visits: Vec<(usize, Location)>,
        x: f32,
        y: f32,
    },
    Status(StatusKind),
    Columns {
        location: Location,
        layout: RwSignal<Layout, LocalStorage>,
        scope: RwSignal<ColumnScope, LocalStorage>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::pages::workspace) enum ColumnScope {
    Location,
    Default,
}

impl ColumnScope {
    pub(in crate::pages::workspace) fn as_value(self) -> &'static str {
        match self {
            Self::Location => "location",
            Self::Default => "default",
        }
    }

    pub(in crate::pages::workspace) fn from_value(value: &str) -> Self {
        match value {
            "default" => Self::Default,
            _ => Self::Location,
        }
    }
}

pub(in crate::pages::workspace) struct Menus {
    pub(in crate::pages::workspace) surface: RwSignal<Surface, LocalStorage>,
}

impl Menus {
    pub(in crate::pages::workspace) fn new() -> Self {
        Self {
            surface: RwSignal::new_local(Surface::Closed),
        }
    }

    pub(in crate::pages::workspace) fn is_open(&self) -> bool {
        !matches!(self.surface.get_untracked(), Surface::Closed)
    }

    pub(in crate::pages::workspace) fn close(&self) {
        self.surface.set(Surface::Closed);
    }
}
