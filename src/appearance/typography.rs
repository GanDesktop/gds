//! Suite-wide font family and text size preferences.
//!
//! The base size drives `--gds-font-size` and scales both `--gds-font-*` and
//! `zgui-ui-tokens` (`--zui-type-*`) typography properties cleanly.

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod fontconfig;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use thiserror::Error;

/// Smallest user-selectable base text size, in pixels.
pub const MIN_SIZE: f32 = 12.0;
/// Largest user-selectable base text size, in pixels.
pub const MAX_SIZE: f32 = 24.0;
/// GDS's default base UI text size; matches `tokens::FONT_SIZE * tokens::UI_TEXT_SCALE` (`14.0` px)
/// or `DEFAULT_SIZE = 16.0` rem base scaled via `scale()`.
pub const DEFAULT_SIZE: f32 = crate::appearance::tokens::FONT_SIZE;
/// Native UI alias, also used if Fontconfig font discovery fails.
const SYSTEM_FAMILY: &str = ".SystemUIFont";
/// Virtual monospace alias used when Fontconfig discovery fails or on hosts without Fontconfig.
const SYSTEM_MONO_FAMILY: &str = ".ZedMono";

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

    /// Resolve these preferences against installed platform fonts without mutating shared state.
    ///
    /// # Errors
    /// Rejects non-finite sizes and values outside [`MIN_SIZE`]..=[`MAX_SIZE`].
    pub fn resolve(&self) -> Result<Resolved, Error> {
        validate_size(self.size)?;
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
            mono_family: system_mono_family(),
            size: self.size,
            unavailable,
        })
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

/// Resolved typography snapshot carried by the reactive appearance service.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub family: String,
    pub mono_family: String,
    pub size: f32,
    pub unavailable: Option<String>,
}

impl Resolved {
    pub fn scale(&self) -> f32 {
        self.size / DEFAULT_SIZE
    }

    pub fn sans_css_stack(&self) -> String {
        css_font_stack(&self.family, "ui-sans-serif, system-ui, sans-serif")
    }

    pub fn mono_css_stack(&self) -> String {
        css_font_stack(&self.mono_family, "ui-monospace, monospace")
    }

    pub(crate) fn write_css_declarations(&self, out: &mut String) {
        let scale = self.scale();
        let _ = writeln!(out, "    --gds-font-sans: {};", self.sans_css_stack());
        let _ = writeln!(out, "    --gds-font-mono: {};", self.mono_css_stack());
        let _ = writeln!(out, "    --gds-font-size: {:.2}px;", self.size);
        let _ = writeln!(out, "    --gds-font-scale: {scale:.4};");
        let _ = writeln!(out, "    --gds-text-2xs: {:.2}px;", 10.0 * scale);
        let _ = writeln!(out, "    --gds-text-xs: {:.2}px;", 12.0 * scale);
        let _ = writeln!(out, "    --gds-text-sm: {:.2}px;", 14.0 * scale);
        let _ = writeln!(out, "    --gds-text-md: {:.2}px;", 16.0 * scale);
        let _ = writeln!(out, "    --gds-text-lg: {:.2}px;", 20.0 * scale);
    }
}

fn css_font_stack(primary: &str, fallback: &str) -> String {
    let clean = primary.replace('"', "");
    if clean.is_empty() {
        fallback.to_owned()
    } else {
        format!("\"{clean}\", {fallback}")
    }
}

struct ResolvedFamily {
    family: String,
    unavailable: Option<String>,
}

impl ResolvedFamily {
    /// An explicit family is only used when it is installed. An empty installed
    /// list means the platform font system could not be queried, so the
    /// configured name is trusted instead of being reported missing.
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

/// Installed font family names reported by the platform font system.
pub fn installed_families() -> Vec<String> {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    return fontconfig::installed_families();
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    Vec::new()
}

pub fn system_family() -> String {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    return fontconfig::system_families().sans_serif.clone();
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    SYSTEM_FAMILY.into()
}

pub fn system_mono_family() -> String {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    return fontconfig::system_families().monospace.clone();
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    SYSTEM_MONO_FAMILY.into()
}

/// Resolve a requested monospace family, falling back to the cached system monospace font.
#[cfg_attr(not(feature = "terminal"), allow(dead_code))]
pub fn resolve_monospace_family(requested: &str) -> String {
    let trimmed = requested.trim();
    if matches!(trimmed, "" | "monospace" | ".SystemMonospace") {
        return system_mono_family();
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    if let Some(matched) = fontconfig::resolve_custom_monospace(trimmed)
        && matched.confirmed_monospace
    {
        return matched.family;
    }

    system_mono_family()
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
