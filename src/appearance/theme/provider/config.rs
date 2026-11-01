//! Read the color scheme explicitly selected in a Qt5ct or Qt6ct configuration.
use directories::BaseDirs;
use gpui_kit::WindowAppearance;
use std::path::PathBuf;

use super::{Error, Loaded, ModePreference, qt, read};

#[derive(Clone, Copy)]
pub(super) enum Qt {
    V5,
    V6,
}

impl Qt {
    fn config_path(self, dirs: &BaseDirs) -> PathBuf {
        let name = match self {
            Self::V5 => "qt5ct",
            Self::V6 => "qt6ct",
        };
        dirs.config_dir().join(name).join(format!("{name}.conf"))
    }

    pub(super) fn source_paths(self) -> Vec<PathBuf> {
        BaseDirs::new()
            .map(|dirs| self.config_path(&dirs))
            .into_iter()
            .collect()
    }

    /// # Errors
    /// Reports missing paths, disabled custom palettes or malformed Qt/KDE color schemes.
    pub(super) fn load(
        self,
        preference: ModePreference,
        platform: WindowAppearance,
    ) -> Result<Loaded, Error> {
        let dirs = BaseDirs::new().ok_or(Error::NoUserDirectories)?;
        let config = self.config_path(&dirs);
        let path = qt::palette_path(&read(&config)?, &config, dirs.home_dir())?;
        let mode = preference.resolve(platform);
        let palette = qt::parse(&read(&path)?, &path, mode)?;
        Ok(Loaded {
            palette,
            mode,
            source_path: Some(path),
        })
    }
}
