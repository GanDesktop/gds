//! Suite-wide font family and text size preferences.
//!
//! The base size is the same value Kit exposes as the theme's `font_size`, so
//! text styled with `rem` units follows it automatically. Explicit pixel sizes
//! cannot, and use [`scaled`] instead.

#[cfg(test)]
mod tests;

use gpui_kit::component::Theme;
use gpui_kit::{App, Global, Pixels, SharedString, px};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Smallest user-selectable base text size, in pixels.
pub const MIN_SIZE: f32 = 12.0;
/// Largest user-selectable base text size, in pixels.
pub const MAX_SIZE: f32 = 24.0;
/// GDS's rem base; ordinary controls use the shared 0.875rem text scale.
pub const DEFAULT_SIZE: f32 = crate::appearance::tokens::FONT_SIZE;
/// Native UI alias, also used if Linux font discovery fails.
const SYSTEM_FAMILY: &str = ".SystemUIFont";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ui", derive(crate::ConfigEditor))]
#[serde(default)]
#[cfg_attr(feature = "ui", config(validate = "Self::validate"))]
pub struct Preferences {
    /// An installed family name; `None` uses the system default.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ui", setting(label = "form-font-family", kind = "font"))]
    pub family: Option<String>,
    /// Base UI text size in pixels.
    #[cfg_attr(feature = "ui", setting(label = "form-font-size", min = MIN_SIZE, max = MAX_SIZE, unit = " px"))]
    pub size: f32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            family: None,
            size: DEFAULT_SIZE,
        }
    }
}

impl Preferences {
    /// # Errors
    /// Rejects a base font size outside the shared supported range.
    pub fn validate(&self) -> Result<(), Error> {
        validate_size(self.size)
    }

    /// Ratio applied to explicit pixel text sizes.
    pub fn scale(&self) -> f32 {
        self.size / DEFAULT_SIZE
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("font size must be finite and between {min} and {max} px, got {value}")]
    InvalidSize { min: f32, max: f32, value: f32 },
}

/// Result of applying [`Preferences`] to the theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// Family handed to the theme; the system family when none was requested.
    pub family: SharedString,
    /// Requested family that is not installed, so the caller can explain why
    /// the system font is drawn instead.
    pub unavailable: Option<String>,
}

/// The typography currently applied, kept so a theme reload can restore it.
#[derive(Clone, Debug)]
struct Current {
    family: SharedString,
    size: f32,
}

impl Global for Current {}

/// Installed font family names reported by the platform text system.
pub(crate) fn installed_families(cx: &App) -> Vec<String> {
    cx.text_system().all_font_names()
}

/// Scale an explicit pixel text size by the shared font size preference.
///
/// `rem`-based styles already follow the theme's base size; this is for fixed
/// pixel sizes that cannot. The default preference returns `base_px` unchanged.
pub(crate) fn scaled(base_px: f32, cx: &App) -> Pixels {
    let scale = cx
        .try_global::<Current>()
        .map_or(1.0, |current| current.size / DEFAULT_SIZE);
    px(base_px * scale)
}

/// Apply the shared font preference to the theme.
///
/// A family that is not installed falls back to the system font and is
/// reported through [`Applied::unavailable`]; the saved preference is kept so
/// installing the font later restores it.
///
/// # Errors
/// Rejects non-finite sizes outside [`MIN_SIZE`]..=[`MAX_SIZE`]. Nothing is
/// changed in that case.
pub(crate) fn install(preferences: &Preferences, cx: &mut App) -> Result<Applied, Error> {
    validate_size(preferences.size)?;
    let resolved = ResolvedFamily::resolve(
        preferences.family.as_deref(),
        &installed_families(cx),
        system_family,
    );
    apply(&resolved.family, preferences.size, cx);
    cx.set_global(Current {
        family: resolved.family.clone(),
        size: preferences.size,
    });
    Ok(Applied {
        family: resolved.family,
        unavailable: resolved.unavailable,
    })
}

/// Re-assert the applied typography after a theme reload resets the fonts.
pub(crate) fn reapply(cx: &mut App) {
    let Some(current) = cx.try_global::<Current>().cloned() else {
        return;
    };
    apply(&current.family, current.size, cx);
}

fn apply(family: &SharedString, size: f32, cx: &mut App) {
    let family = family.clone();
    Theme::update(cx, move |theme| {
        theme.font_family = family;
        theme.font_size = px(size);
    });
}

struct ResolvedFamily {
    family: SharedString,
    unavailable: Option<String>,
}

impl ResolvedFamily {
    /// An explicit family is only used when it is installed. An empty installed
    /// list means the platform font system could not be queried, so the
    /// configured name is trusted instead of being reported missing.
    fn resolve(
        requested: Option<&str>,
        installed: &[String],
        system_family: impl FnOnce() -> SharedString,
    ) -> Self {
        match requested {
            Some(family)
                if installed.is_empty() || installed.iter().any(|name| name.as_str() == family) =>
            {
                Self {
                    family: family.into(),
                    unavailable: None,
                }
            }
            Some(family) => Self {
                family: system_family(),
                unavailable: Some(family.to_owned()),
            },
            None => Self {
                family: system_family(),
                unavailable: None,
            },
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn system_family() -> SharedString {
    system_alias("sans-serif", SYSTEM_FAMILY)
}

#[cfg(target_os = "linux")]
pub(super) fn system_mono_family() -> SharedString {
    system_alias("monospace", ".ZedMono")
}

#[cfg(target_os = "linux")]
fn system_alias(alias: &str, fallback: &str) -> SharedString {
    use font_kit::source::SystemSource;

    // Resolve Fontconfig aliases to real families before Kit probes its defaults.
    let family = match SystemSource::new().select_family_by_name(alias) {
        Ok(family) => family,
        Err(error) => {
            tracing::warn!(alias, %error, "cannot resolve the Fontconfig font; using GPUI fallback");
            return fallback.into();
        }
    };
    let mut last_error = None;
    for handle in family.fonts() {
        match handle.load() {
            Ok(face) => return face.family_name().into(),
            Err(error) => last_error = Some(error),
        }
    }
    tracing::warn!(alias, error = ?last_error, "cannot load the Fontconfig font; using GPUI fallback");
    fallback.into()
}

#[cfg(not(target_os = "linux"))]
fn system_family() -> SharedString {
    SYSTEM_FAMILY.into()
}

/// # Errors
/// Rejects non-finite sizes and values outside [`MIN_SIZE`]..=[`MAX_SIZE`].
fn validate_size(size: f32) -> Result<(), Error> {
    if size.is_finite() && (MIN_SIZE..=MAX_SIZE).contains(&size) {
        Ok(())
    } else {
        Err(Error::InvalidSize {
            min: MIN_SIZE,
            max: MAX_SIZE,
            value: size,
        })
    }
}
