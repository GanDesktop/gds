//! Pure workspace UI flags, sidebar selection descriptors, and window launch options.

use std::{path::PathBuf, rc::Rc};

use crate::{
    entities::location::{
        LocationTarget,
        places::{Error as PlacesError, Places},
        volumes::VolumeId,
    },
    features::pick_files,
};

pub(crate) struct LaunchOptions {
    pub places: Result<Places, PlacesError>,
    pub start: PathBuf,
    pub home: PathBuf,
    pub picker: Option<pick_files::Options>,
}

pub(in crate::pages::workspace) type OpenPickerWindow =
    Rc<dyn Fn(String, LaunchOptions, Rc<dyn Fn(Vec<PathBuf>)>)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::pages::workspace) enum FilterFocusMode {
    SelectAll,
    CaretEnd,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::pages::workspace) enum SidebarRegion {
    Tabs,
    QuickAccess,
    Locations,
    Volumes,
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::pages::workspace) enum SidebarRow {
    Tab(u64),
    QuickAccess(PathBuf),
    Location(LocationTarget),
    Volume(VolumeId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::pages::workspace) enum CollapsibleSection {
    QuickAccess,
    Volumes,
    Locations,
}

impl CollapsibleSection {
    pub(in crate::pages::workspace) fn from_region(region: SidebarRegion) -> Option<Self> {
        match region {
            SidebarRegion::Tabs => None,
            SidebarRegion::QuickAccess => Some(Self::QuickAccess),
            SidebarRegion::Volumes => Some(Self::Volumes),
            SidebarRegion::Locations => Some(Self::Locations),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::pages::workspace) enum AccordionSelection {
    #[default]
    Auto,
    Expanded(CollapsibleSection),
    AllCollapsed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::pages::workspace) enum QuickAccessAddState {
    CanAdd,
    AlreadyAdded,
    NotLocal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::pages::workspace) enum StatusKind {
    Tasks,
    Selection,
    Clipboard,
}

impl StatusKind {
    pub(in crate::pages::workspace) fn title(self) -> &'static str {
        match self {
            Self::Tasks => "tasks-title",
            Self::Selection => "selection-title",
            Self::Clipboard => "clipboard-title",
        }
    }

    pub(in crate::pages::workspace) fn empty(self) -> &'static str {
        match self {
            Self::Tasks => "tasks-empty",
            Self::Selection => "selection-empty",
            Self::Clipboard => "clipboard-empty",
        }
    }
}
