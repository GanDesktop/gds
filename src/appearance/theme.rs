//! Appearance preferences, palette installation, materials, and live updates.

pub(crate) mod color;
mod hot_reload;
mod material;
mod palette;
mod terminal;
pub use terminal::TerminalColors;
pub mod provider;
#[cfg(test)]
mod tests;

pub use super::mode::{Contrast, Mode};
pub use hot_reload::HotReload;
pub use material::Material;
pub use palette::Palette as Colors;
pub use provider::{Source, builtin::Theme as BuiltinTheme};

/// Stylesheet identifier used when installing `--zui-*` and `--gds-*` tokens into a window.
pub const APPEARANCE_SHEET: &str = "gds-appearance";

use std::fmt::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zgui_ui_tokens::{Theme as TokensTheme, theme_sheet};

use super::{motion, tokens, typography};
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
#[serde(rename_all = "lowercase")]
pub enum ModePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ModePreference {
    /// Resolve a platform-backed preference without changing the stored choice.
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
        let opacity = tokens::SurfaceOpacity::default();
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
    appearance: Appearance,
    loaded: Loaded,
    contrast: Contrast,
}

impl Prepared {
    pub(crate) fn set_opacity(&mut self, content: f32, sidebar: f32) -> Result<(), Error> {
        validate_opacity("content_opacity", content)?;
        validate_opacity("sidebar_opacity", sidebar)?;
        self.appearance.content_opacity = content;
        self.appearance.sidebar_opacity = sidebar;
        Ok(())
    }

    pub(crate) fn set_rounded_corners(&mut self, rounded: bool) {
        self.appearance.rounded_corners = rounded;
    }
    /// Files affecting this palette, including the active source file resolved during loading.
    pub fn source_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.appearance.source_paths();
        if let Some(path) = &self.loaded.source_path {
            paths.push(path.clone());
        }
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

    pub fn contrast(&self) -> Contrast {
        self.contrast
    }

    pub fn appearance(&self) -> &Appearance {
        &self.appearance
    }

    pub fn material(&self) -> Material {
        let (content_opacity, sidebar_opacity) = if self.contrast.is_more() {
            (1.0, 1.0)
        } else {
            (
                self.appearance.content_opacity,
                self.appearance.sidebar_opacity,
            )
        };
        Material {
            content: self.loaded.palette.content,
            sidebar: self.loaded.palette.sidebar,
            content_opacity,
            sidebar_opacity,
        }
    }

    pub fn applied(&self) -> Applied {
        Applied {
            source: self.appearance.source.clone(),
            mode: self.loaded.mode,
            source_path: self.loaded.source_path.clone(),
        }
    }

    pub fn tokens_theme(
        &self,
        typography: &typography::Resolved,
        motion: motion::Policy,
    ) -> TokensTheme {
        self.loaded.palette.tokens_theme(
            self.loaded.mode,
            typography,
            motion,
            self.appearance.rounded_corners,
            self.contrast,
        )
    }

    /// Generate the `:root` Stylo CSS sheet containing all `--gds-*` custom properties.
    pub fn gds_stylesheet_css(
        &self,
        typography: &typography::Resolved,
        motion: motion::Policy,
    ) -> String {
        let mut out = String::from(":root {\n");
        let _ = writeln!(out, "    color-scheme: {};", self.loaded.mode.css_keyword());
        let _ = writeln!(out, "    --gds-contrast: {};", self.contrast.css_keyword());
        let window_radius = if self.appearance.rounded_corners {
            tokens::WINDOW_CORNER_RADIUS
        } else {
            0.0
        };
        let _ = writeln!(out, "    --gds-radius-window: {window_radius}px;");
        let _ = writeln!(
            out,
            "    --gds-radius-control: {}px;",
            tokens::DataListStyle::ROW_RADIUS
        );
        self.loaded
            .palette
            .write_css_declarations(self.contrast, &mut out);
        self.material().write_css_declarations(&mut out);
        tokens::write_css_declarations(&mut out, typography.scale());
        typography.write_css_declarations(&mut out);
        motion.write_css_declarations(&mut out);
        out.push_str("}\n");
        out
    }

    /// Generate the combined Stylo CSS sheet containing `--zui-*` and `--gds-*` tokens.
    pub fn full_stylesheet_css(
        &self,
        typography: &typography::Resolved,
        motion: motion::Policy,
    ) -> String {
        let tokens = self.tokens_theme(typography, motion);
        let zui = theme_sheet(":root", &tokens, &tokens, self.loaded.mode.color_scheme());
        let gds = self.gds_stylesheet_css(typography, motion);
        format!("{zui}\n{gds}")
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
    /// Prime asynchronous desktop session state (XDG Settings Portal color scheme, accent, and
    /// high-contrast preference) before resolving the initial theme synchronously on the UI thread.
    pub async fn prime_session(&self) {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        super::portal::prime().await;
    }

    /// Provider inputs; a prepared palette adds its resolved palette file.
    pub fn source_paths(&self) -> Vec<PathBuf> {
        self.source.source_paths()
    }

    pub(crate) fn platform_mode() -> Mode {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        if let Some(mode) = super::portal::snapshot().color_scheme {
            return mode;
        }
        Mode::Dark
    }

    pub(crate) fn platform_contrast() -> Contrast {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            super::portal::snapshot().contrast
        }
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            Contrast::Normal
        }
    }

    /// Read and validate one complete palette for a known platform appearance.
    ///
    /// # Errors
    /// Reports invalid opacity, unavailable source files or malformed palette data.
    pub fn prepare(&self, window_appearance: Mode) -> Result<Prepared, Error> {
        self.prepare_with_contrast(window_appearance, Self::platform_contrast())
    }

    /// Read and validate one complete palette using an explicit contrast preference.
    ///
    /// # Errors
    /// Reports invalid opacity, unavailable source files or malformed palette data.
    pub fn prepare_with_contrast(
        &self,
        window_appearance: Mode,
        contrast: Contrast,
    ) -> Result<Prepared, Error> {
        validate_opacity("content_opacity", self.content_opacity)?;
        validate_opacity("sidebar_opacity", self.sidebar_opacity)?;
        let mut loaded = self.load_for(window_appearance)?;
        if contrast.is_more() {
            loaded.palette.enhance_contrast(loaded.mode);
        }
        Ok(Prepared {
            appearance: self.clone(),
            loaded,
            contrast,
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
