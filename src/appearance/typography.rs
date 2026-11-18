//! Suite typography preferences, resolved through the platform font catalogue.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(test)]
mod tests;
use serde::{Deserialize, Serialize};
use thiserror::Error;
const SYSTEM_FAMILY: &str = "sans-serif";
const SYSTEM_MONO_FAMILY: &str = "monospace";

/// Smallest user-selectable base text size, in pixels.
pub const MIN_SIZE: f32 = 12.0;
/// Largest user-selectable base text size, in pixels.
pub const MAX_SIZE: f32 = 24.0;
/// GDS's rem base; ordinary controls use the shared 0.875rem text scale.
pub const DEFAULT_SIZE: f32 = crate::appearance::tokens::FONT_SIZE;

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
    pub family: String,
    /// Requested family that is not installed, so the caller can explain why
    /// the system font is drawn instead.
    pub unavailable: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Resolved {
    pub family: String,
    pub size: f32,
    pub unavailable: Option<String>,
}
impl Preferences {
    pub(crate) fn resolve(&self) -> Result<Resolved, Error> {
        self.validate()?;
        let installed = self
            .family
            .as_ref()
            .map(|_| installed_families())
            .unwrap_or_default();
        let ResolvedFamily {
            family,
            unavailable,
        } = ResolvedFamily::resolve(self.family.as_deref(), &installed, system_family);
        Ok(Resolved {
            family,
            size: self.size,
            unavailable,
        })
    }
}

struct ResolvedFamily {
    family: String,
    unavailable: Option<String>,
}
impl ResolvedFamily {
    fn resolve(
        requested: Option<&str>,
        installed: &[String],
        fallback: impl FnOnce() -> String,
    ) -> Self {
        match requested {
            Some(family) if installed.is_empty() || installed.iter().any(|name| name == family) => {
                Self {
                    family: family.into(),
                    unavailable: None,
                }
            }
            Some(family) => Self {
                family: fallback(),
                unavailable: Some(family.into()),
            },
            None => Self {
                family: fallback(),
                unavailable: None,
            },
        }
    }
}

pub fn installed_families() -> Vec<String> {
    #[cfg(target_os = "linux")]
    return linux::installed_families();
    #[cfg(not(target_os = "linux"))]
    Vec::new()
}
pub fn system_family() -> String {
    #[cfg(target_os = "linux")]
    return linux::system_families().sans_serif.clone();
    #[cfg(not(target_os = "linux"))]
    SYSTEM_FAMILY.into()
}
pub fn system_mono_family() -> String {
    #[cfg(target_os = "linux")]
    return linux::system_families().monospace.clone();
    #[cfg(not(target_os = "linux"))]
    SYSTEM_MONO_FAMILY.into()
}
pub fn resolve_monospace_family(requested: &str) -> String {
    let requested = requested.trim();
    if matches!(requested, "" | "monospace" | ".SystemMonospace") {
        return system_mono_family();
    }
    #[cfg(target_os = "linux")]
    if let Some(matched) = linux::resolve_custom_monospace(requested)
        && matched.confirmed_monospace
    {
        return matched.family;
    }
    system_mono_family()
}
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
