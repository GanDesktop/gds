mod uri;

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// A content location, independent of the tab or sidebar entry presenting it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Location {
    Directory(OsString),
    Remote(gds::service::remote::Address),
    Recent,
    Favorites,
    Network,
    Trash,
    Volumes,
    Volume {
        identity: VolumeIdentity,
        relative: OsString,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum VolumeIdentity {
    Stable(String),
    Attachment(String),
}

impl Location {
    pub(crate) fn display_address(&self) -> String {
        match self {
            Self::Directory(path) => Path::new(path).display().to_string(),
            _ => self
                .uri()
                .expect("non-filesystem locations always have a URI"),
        }
    }

    pub(crate) fn local_path(&self) -> Option<&Path> {
        match self {
            Self::Directory(path) => Some(Path::new(path)),
            _ => None,
        }
    }

    pub(crate) fn virtual_uri(&self) -> Option<&'static str> {
        match self {
            Self::Recent => Some("location://recent"),
            Self::Favorites => Some("location://favorites"),
            Self::Network => Some("location://network"),
            Self::Trash => Some("location://trash"),
            Self::Volumes => Some("location://volumes"),
            _ => None,
        }
    }

    pub(crate) fn from_virtual_uri(input: &str) -> Option<Self> {
        match input.strip_suffix('/').unwrap_or(input) {
            "location://recent" => Some(Self::Recent),
            "location://favorites" => Some(Self::Favorites),
            "location://network" => Some(Self::Network),
            "location://trash" => Some(Self::Trash),
            "location://volumes" => Some(Self::Volumes),
            _ => None,
        }
    }
}

impl From<&Path> for Location {
    fn from(path: &Path) -> Self {
        Self::Directory(path.as_os_str().to_owned())
    }
}
impl From<&PathBuf> for Location {
    fn from(path: &PathBuf) -> Self {
        Self::from(path.as_path())
    }
}
impl From<&Location> for Location {
    fn from(location: &Location) -> Self {
        location.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ViewScope {
    Defaults,
    Location(Location),
}

impl From<OsString> for Location {
    fn from(path: OsString) -> Self {
        Self::Directory(path)
    }
}

impl From<&str> for Location {
    fn from(path: &str) -> Self {
        Self::from(Path::new(path))
    }
}
