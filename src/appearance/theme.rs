//! Appearance preferences, palette installation, materials and live updates.

mod hot_reload;
mod palette;
mod terminal;
pub use terminal::TerminalColors;
pub mod provider;
#[cfg(test)]
mod tests;

pub use hot_reload::HotReload;
pub use palette::Palette as Colors;
pub use provider::{Source, builtin::Theme as BuiltinTheme};

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use provider::Loaded;

/// Source category shown separately from the selected palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceKind {
    #[default]
    System,
    Builtin,
    Custom,
}

impl From<&Source> for SourceKind {
    fn from(source: &Source) -> Self {
        match source {
            Source::System => Self::System,
            Source::Builtin => Self::Builtin,
            Source::Custom(_) => Self::Custom,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Light,
    #[default]
    Dark,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ModePreference {
    pub fn resolve(self, platform: Mode) -> Mode {
        match self {
            Self::Light => Mode::Light,
            Self::Dark => Mode::Dark,
            Self::System => platform,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Appearance {
    pub source: Source,
    /// Preserved when using an external source; defaults to Bamboo for old settings.
    pub builtin: BuiltinTheme,
    /// System follows the selected source; Light/Dark remain explicit overrides.
    pub mode: ModePreference,
    pub content_opacity: f32,
    pub sidebar_opacity: f32,
    pub rounded_corners: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        let opacity = crate::appearance::tokens::SurfaceOpacity::default();
        Self {
            source: Source::default(),
            builtin: BuiltinTheme::default(),
            mode: ModePreference::default(),
            content_opacity: opacity.content,
            sidebar_opacity: opacity.sidebar,
            rounded_corners: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub source: Source,
    pub mode: Mode,
    /// The resolved palette file read by the active source, when file-backed.
    pub source_path: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not determine the user's XDG directories")]
    NoUserDirectories,
    #[error("no supported desktop shell is active")]
    UnsupportedDesktop,
    #[error("could not read theme file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid theme JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid theme INI in {path}: {source}")]
    Ini {
        path: PathBuf,
        #[source]
        source: ini::ParseError,
    },
    #[error("theme field {field}: {detail}")]
    Invalid { field: String, detail: String },
    #[error("{name} must be finite and between 0 and 1, got {value}")]
    InvalidOpacity { name: &'static str, value: f32 },
    #[error("apply an appearance before updating its opacity")]
    MaterialNotInitialized,
}

impl Error {
    fn invalid(field: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Invalid {
            field: field.into(),
            detail: detail.into(),
        }
    }
}

/// A validated palette that can be read away from the UI thread.
/// Applications can compare snapshots before installing external changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub(crate) appearance: Appearance,
    loaded: Loaded,
}

impl Prepared {
    /// Files that can change this prepared appearance.
    pub fn source_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.appearance.source_paths();
        paths.extend(self.loaded.source_path.clone());
        paths.sort();
        paths.dedup();
        paths
    }
    pub fn colors(&self) -> Colors {
        self.loaded.palette
    }
    pub fn mode(&self) -> Mode {
        self.loaded.mode
    }
    pub fn appearance(&self) -> &Appearance {
        &self.appearance
    }
    pub fn applied(&self) -> Applied {
        Applied {
            source: self.appearance.source.clone(),
            mode: self.loaded.mode,
            source_path: self.loaded.source_path.clone(),
        }
    }
}

/// # Errors
/// Rejects non-finite alpha and values outside the supported range.
pub(crate) fn validate_opacity(name: &'static str, value: f32) -> Result<(), Error> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(Error::InvalidOpacity { name, value })
    }
}

impl Appearance {
    /// Prime asynchronous desktop session state (such as the XDG Settings Portal on GNOME)
    /// before resolving the initial theme synchronously on the UI thread.
    pub async fn prime_session(&self) {
        #[cfg(target_os = "linux")]
        if self.source == Source::System || self.mode == ModePreference::System {
            super::portal::prime().await;
        }
    }

    /// Provider inputs; a prepared palette adds its resolved palette file.
    pub fn source_paths(&self) -> Vec<PathBuf> {
        self.source.source_paths()
    }
    /// Read and validate one complete palette for a known platform appearance.
    ///
    /// # Errors
    /// Reports invalid opacity, unavailable source files or malformed palette data.
    pub fn prepare(&self, window_appearance: Mode) -> Result<Prepared, Error> {
        validate_opacity("content_opacity", self.content_opacity)?;
        validate_opacity("sidebar_opacity", self.sidebar_opacity)?;
        Ok(Prepared {
            appearance: self.clone(),
            loaded: self.load_for(window_appearance)?,
        })
    }

    /// Fall back to a local palette while preserving independent appearance choices.
    /// Only invalid opacity values are replaced by safe defaults.
    pub fn builtin_fallback(&self) -> Self {
        let defaults = Self::default();
        Self {
            source: Source::Builtin,
            content_opacity: if validate_opacity("content_opacity", self.content_opacity).is_ok() {
                self.content_opacity
            } else {
                defaults.content_opacity
            },
            sidebar_opacity: if validate_opacity("sidebar_opacity", self.sidebar_opacity).is_ok() {
                self.sidebar_opacity
            } else {
                defaults.sidebar_opacity
            },
            ..self.clone()
        }
    }

    /// Resolve the selected source against a known platform appearance.
    ///
    /// # Errors
    /// Propagates source discovery, file access and palette decoding failures.
    fn load_for(&self, window_appearance: Mode) -> Result<Loaded, Error> {
        self.source.load(self.mode, window_appearance, self.builtin)
    }
}
