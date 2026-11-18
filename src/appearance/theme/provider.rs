//! Palette providers grouped by system state, configured files and built-in data.
pub mod builtin;
mod color;
pub mod custom;
pub(crate) mod system;

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use super::{Error, Mode, ModePreference, palette::Palette};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A TOML palette relative to the shared GDS themes directory.
    Custom(PathBuf),
    /// An embedded palette; System mode follows the native platform appearance.
    Builtin,
    /// The current desktop's palette: GNOME, KDE Plasma, Noctalia or DankMaterialShell.
    #[default]
    #[serde(alias = "dms")]
    System,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Loaded {
    pub palette: Palette,
    pub mode: Mode,
    pub source_path: Option<PathBuf>,
}

impl Source {
    /// Offer desktop following only when its palette can actually be loaded.
    /// Call when opening or refreshing settings, not on each render or slider update.
    pub fn system_available(platform: Mode) -> bool {
        system::load(ModePreference::System, platform).is_ok()
    }

    pub(super) fn source_paths(&self) -> Vec<PathBuf> {
        match self {
            Self::Builtin => Vec::new(),
            Self::Custom(path) => custom::resolve(path).into_iter().collect(),
            Self::System => system::source_paths(),
        }
    }

    /// # Errors
    /// Reports unavailable files or invalid provider data without substituting another source.
    pub(super) fn load(
        &self,
        preference: ModePreference,
        platform: Mode,
        builtin: builtin::Theme,
    ) -> Result<Loaded, Error> {
        match self {
            Self::Builtin => Ok(builtin::load(builtin, preference, platform)),
            Self::Custom(path) => custom::load(path, preference.resolve(platform)),
            Self::System => system::load(preference, platform),
        }
    }
}

/// # Errors
/// Reports inaccessible files and non-UTF-8 theme content with their path.
fn read(path: &std::path::Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_owned(),
        source,
    })
}
