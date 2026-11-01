use toasty::{Embed, Json, Model};

use crate::entities::file::x::location::{Layout, ViewMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Embed)]
#[column(type = text, rename_all = "snake_case")]
pub(super) enum Kind {
    Favorite,
    Recent,
    Pin,
}

#[derive(Debug, Model)]
#[table = "locations"]
#[key(kind, path)]
pub(super) struct Location {
    pub kind: Kind,
    pub path: Vec<u8>,
    pub position: i64,
}

#[derive(Debug, Model)]
#[table = "saved_locations"]
#[key(kind, location_kind, identity, path)]
pub(super) struct SavedLocation {
    pub kind: Kind,
    pub location_kind: ViewKind,
    pub identity: String,
    pub path: Vec<u8>,
    pub position: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Embed)]
#[column(type = text, rename_all = "snake_case")]
pub(super) enum View {
    List,
    Grid,
}

impl From<ViewMode> for View {
    fn from(mode: ViewMode) -> Self {
        match mode {
            ViewMode::List => Self::List,
            ViewMode::Grid => Self::Grid,
        }
    }
}

impl From<View> for ViewMode {
    fn from(mode: View) -> Self {
        match mode {
            View::List => Self::List,
            View::Grid => Self::Grid,
        }
    }
}

#[derive(Debug, Model)]
#[table = "view_settings"]
#[key(kind, identity, path)]
pub(super) struct ViewSettings {
    #[column("key_kind")]
    pub kind: ViewKind,
    #[column("key_identity")]
    pub identity: String,
    #[column("key_path")]
    pub path: Vec<u8>,
    #[column(type = text)]
    pub columns: Option<Json<Layout>>,
    #[column(type = text)]
    pub widths: Option<Json<crate::entities::location::ColumnWidths>>,
    #[column(type = text)]
    pub sort: Option<Json<crate::entities::file::x::location::Sort>>,
    pub details: Option<bool>,
    pub view_mode: Option<View>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Embed)]
#[column(type = text, rename_all = "snake_case")]
pub(super) enum ViewKind {
    Defaults,
    Directory,
    Remote,
    Recent,
    Favorites,
    Network,
    Trash,
    Volumes,
    VolumeStable,
    VolumeAttachment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ViewKey {
    pub kind: ViewKind,
    pub identity: String,
    pub path: Vec<u8>,
}

impl ViewKey {
    /// # Errors
    /// Reports failures encoding a non-lossy OS path.
    pub(super) fn new(scope: &crate::entities::location::ViewScope) -> Result<Self, super::Error> {
        use crate::entities::location::{Location, ViewScope, VolumeIdentity};
        let mut key = Self {
            kind: ViewKind::Defaults,
            identity: String::new(),
            path: Vec::new(),
        };
        key.kind = match scope {
            ViewScope::Defaults => ViewKind::Defaults,
            ViewScope::Location(location) => match location {
                Location::Directory(path) => {
                    key.path = serde_json::to_vec(path)?;
                    ViewKind::Directory
                }
                Location::Remote(address) => {
                    key.identity = address.uri().to_owned();
                    ViewKind::Remote
                }
                Location::Recent => ViewKind::Recent,
                Location::Favorites => ViewKind::Favorites,
                Location::Network => ViewKind::Network,
                Location::Trash => ViewKind::Trash,
                Location::Volumes => ViewKind::Volumes,
                Location::Volume { identity, relative } => {
                    key.path = serde_json::to_vec(relative)?;
                    match identity {
                        VolumeIdentity::Stable(identity) => {
                            key.identity = identity.clone();
                            ViewKind::VolumeStable
                        }
                        VolumeIdentity::Attachment(identity) => {
                            key.identity = identity.clone();
                            ViewKind::VolumeAttachment
                        }
                    }
                }
            },
        };
        Ok(key)
    }

    /// # Errors
    /// Reports malformed persisted OS paths.
    pub(super) fn into_scope(self) -> Result<crate::entities::location::ViewScope, super::Error> {
        use crate::entities::location::{Location, ViewScope, VolumeIdentity};
        let location = match self.kind {
            ViewKind::Defaults => return Ok(ViewScope::Defaults),
            ViewKind::Directory => Location::Directory(serde_json::from_slice(&self.path)?),
            ViewKind::Remote => Location::Remote(
                gds::service::remote::Address::parse(&self.identity)
                    .map_err(|error| super::Error::Remote(error.to_string()))?,
            ),
            ViewKind::Recent => Location::Recent,
            ViewKind::Favorites => Location::Favorites,
            ViewKind::Network => Location::Network,
            ViewKind::Trash => Location::Trash,
            ViewKind::Volumes => Location::Volumes,
            ViewKind::VolumeStable | ViewKind::VolumeAttachment => Location::Volume {
                identity: if self.kind == ViewKind::VolumeStable {
                    VolumeIdentity::Stable(self.identity)
                } else {
                    VolumeIdentity::Attachment(self.identity)
                },
                relative: serde_json::from_slice(&self.path)?,
            },
        };
        Ok(ViewScope::Location(location))
    }
}
