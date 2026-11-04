//! Palette providers grouped by system state, configured files and built-in data.
pub mod builtin;
mod color;
mod config;
mod qt;
mod system;

use gpui_kit::WindowAppearance;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use super::{Error, Mode, ModePreference, palette::Palette};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// An embedded palette; System mode follows the native platform appearance.
    Builtin,
    /// Palette selected by the user's Qt6ct configuration.
    Qt6ct,
    /// Palette selected by the user's Qt5ct configuration.
    Qt5ct,
    /// The current desktop's palette: GNOME, KDE Plasma or DankMaterialShell.
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
    pub(super) fn source_paths(self) -> Vec<PathBuf> {
        match self {
            Self::Builtin => Vec::new(),
            Self::System => system::source_paths(),
            Self::Qt5ct => config::Qt::V5.source_paths(),
            Self::Qt6ct => config::Qt::V6.source_paths(),
        }
    }

    /// Whether System mode is supplied by the native platform rather than the source.
    pub(super) fn uses_platform_mode(self) -> bool {
        !matches!(self, Self::System) || system::uses_platform_mode()
    }

    /// # Errors
    /// Reports unavailable files or invalid provider data without substituting another source.
    pub(super) fn load(
        self,
        preference: ModePreference,
        platform: WindowAppearance,
        builtin: builtin::Theme,
    ) -> Result<Loaded, Error> {
        match self {
            Self::Builtin => Ok(builtin::load(builtin, preference, platform)),
            Self::System => system::load(preference, platform),
            Self::Qt5ct => config::Qt::V5.load(preference, platform),
            Self::Qt6ct => config::Qt::V6.load(preference, platform),
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
