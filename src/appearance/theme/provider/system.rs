//! Follow KDE Plasma's active palette or DankMaterialShell's exported schemes.
mod dms;

use directories::BaseDirs;
use std::path::PathBuf;

use super::{Error, Loaded, ModePreference, qt, read};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Desktop {
    Dms,
    Kde,
}

impl Desktop {
    fn current() -> Self {
        Self::from_session(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
    }

    fn from_session(desktop: &str) -> Self {
        if desktop
            .split(':')
            .any(|name| name.eq_ignore_ascii_case("KDE"))
        {
            Self::Kde
        } else {
            Self::Dms
        }
    }

    fn path(self, dirs: &BaseDirs) -> PathBuf {
        match self {
            Self::Kde => dirs.config_dir().join("kdeglobals"),
            Self::Dms => dirs.cache_dir().join("DankMaterialShell/dms-colors.json"),
        }
    }
}

pub(super) fn source_paths() -> Vec<PathBuf> {
    BaseDirs::new()
        .map(|dirs| Desktop::current().path(&dirs))
        .into_iter()
        .collect()
}

/// # Errors
/// Reports missing user directories, unreadable files or invalid desktop palettes.
pub(super) fn load(preference: ModePreference) -> Result<Loaded, Error> {
    let dirs = BaseDirs::new().ok_or(Error::NoUserDirectories)?;
    let desktop = Desktop::current();
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
    }
}

#[cfg(test)]
mod tests;
