//! Presentation metadata and collection groupings for saved and virtual locations.

use std::path::Path;

use gpui_kit::{App, assets::IconName};

use super::{Location, scheme::TagAddress};
use crate::shared::i18n;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Collection {
    Tags,
    Recent,
    Favorites,
    Network,
    Trash,
    Volumes,
}

impl Collection {
    pub fn from_location(location: &Location) -> Option<Self> {
        match location {
            Location::Tags(_) => Some(Self::Tags),
            Location::Recent => Some(Self::Recent),
            Location::Favorites => Some(Self::Favorites),
            Location::Network => Some(Self::Network),
            Location::Trash => Some(Self::Trash),
            Location::Volumes => Some(Self::Volumes),
            _ => None,
        }
    }

    pub fn location(self) -> Location {
        match self {
            Self::Tags => Location::Tags(TagAddress::new("").expect("root tag address")),
            Self::Recent => Location::Recent,
            Self::Favorites => Location::Favorites,
            Self::Volumes => Location::Volumes,
            Self::Network => Location::Network,
            Self::Trash => Location::Trash,
        }
    }

    pub const fn label_id(self) -> &'static str {
        match self {
            Self::Tags => "tags-title",
            Self::Recent => "recent",
            Self::Favorites => "favorites",
            Self::Volumes => "volumes",
            Self::Network => "network",
            Self::Trash => "trash",
        }
    }

    pub const fn icon(self) -> IconName {
        match self {
            Self::Tags => IconName::Tag,
            Self::Recent => IconName::Clock,
            Self::Favorites => IconName::Star,
            Self::Volumes => IconName::HardDrive,
            Self::Network => IconName::Network,
            Self::Trash => IconName::Trash,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocationTarget {
    Home,
    Collection(Collection),
}

impl LocationTarget {
    pub const ALL: [Self; 7] = [
        Self::Home,
        Self::Collection(Collection::Tags),
        Self::Collection(Collection::Recent),
        Self::Collection(Collection::Favorites),
        Self::Collection(Collection::Volumes),
        Self::Collection(Collection::Network),
        Self::Collection(Collection::Trash),
    ];

    pub fn location(self, home: &Path) -> Location {
        match self {
            Self::Home => Location::from(home),
            Self::Collection(collection) => collection.location(),
        }
    }

    pub const fn label_id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Collection(collection) => collection.label_id(),
        }
    }

    pub const fn icon(self) -> IconName {
        match self {
            Self::Home => IconName::House,
            Self::Collection(collection) => collection.icon(),
        }
    }
}

impl Location {
    pub(crate) fn collection(&self) -> Option<Collection> {
        Collection::from_location(self)
    }

    pub(crate) fn icon(&self) -> IconName {
        match self {
            Self::Directory(_) => IconName::Folder,
            Self::Remote(_) => IconName::Network,
            Self::Volume { .. } => IconName::HardDrive,
            _ => self
                .collection()
                .expect("non-path location belongs to a collection")
                .icon(),
        }
    }

    pub(crate) fn label(&self, cx: &App) -> String {
        match self {
            Self::Tags(address) if !address.tag().is_empty() => address.label().to_owned(),
            Self::Remote(address) => address.name(),
            Self::Directory(path) => Path::new(path)
                .file_name()
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned(),
            Self::Volume { relative, .. } if !relative.is_empty() => Path::new(relative)
                .file_name()
                .unwrap_or(relative)
                .to_string_lossy()
                .into_owned(),
            Self::Volume { .. } => i18n::text(Collection::Volumes.label_id(), cx).to_string(),
            _ => {
                let collection = self
                    .collection()
                    .expect("non-path location belongs to a collection");
                i18n::text(collection.label_id(), cx).to_string()
            }
        }
    }
}
