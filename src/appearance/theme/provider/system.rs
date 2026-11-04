//! Follow the active desktop's palette and appearance preferences.
mod dms;
mod gnome;

use directories::BaseDirs;
use gpui_kit::WindowAppearance;
use std::path::PathBuf;

use super::{Error, Loaded, ModePreference, qt, read};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Desktop {
    Dms,
    Kde,
    Gnome,
}

impl Desktop {
    fn current() -> Self {
        Self::from_session(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
    }

    fn from_session(desktop: &str) -> Self {
        for name in desktop.split(':') {
            if name.eq_ignore_ascii_case("KDE") {
                return Self::Kde;
            }
            if name.eq_ignore_ascii_case("GNOME") {
                return Self::Gnome;
            }
        }
        Self::Dms
    }

    fn path(self, dirs: &BaseDirs) -> PathBuf {
        match self {
            Self::Kde => dirs.config_dir().join("kdeglobals"),
            Self::Dms => dirs.cache_dir().join("DankMaterialShell/dms-colors.json"),
            Self::Gnome => dirs.config_dir().join("dconf/user"),
        }
    }
}

pub(super) fn uses_platform_mode() -> bool {
    Desktop::current() == Desktop::Gnome
}

pub(super) fn source_paths() -> Vec<PathBuf> {
    BaseDirs::new()
        .map(|dirs| {
            let desktop = Desktop::current();
            let mut paths = vec![desktop.path(&dirs)];
            if desktop == Desktop::Gnome {
                paths.push(dirs.config_dir().join("gtk-3.0/gtk.css"));
                paths.push(dirs.config_dir().join("gtk-3.0/settings.ini"));
            }
            paths
        })
        .unwrap_or_default()
}

/// # Errors
/// Reports missing user directories, unreadable files or invalid desktop palettes.
pub(super) fn load(
    preference: ModePreference,
    platform: WindowAppearance,
) -> Result<Loaded, Error> {
    let dirs = BaseDirs::new().ok_or(Error::NoUserDirectories)?;
    let desktop = Desktop::current();
    if desktop == Desktop::Gnome {
        return gnome::load(preference, platform);
    }
    let path = desktop.path(&dirs);
    let contents = read(&path)?;
    match desktop {
        Desktop::Kde => qt::parse_kde_system(&contents, &path, preference),
        Desktop::Dms => {
            let parsed = dms::parse(&contents, &path, preference)?;
            Ok(Loaded {
                palette: parsed.palette,
                mode: parsed.mode,
                source_path: Some(path),
            })
        }
        Desktop::Gnome => unreachable!("GNOME uses GSettings rather than a palette file"),
    }
}

#[cfg(test)]
mod tests;
