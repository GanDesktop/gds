//! Serializable preference schema persisted in `gds/app/images/settings.toml`.

#[cfg(test)]
mod tests;

use gds::appearance::{theme, typography};
use serde::{Deserialize, Serialize};

pub const DEFAULT_JPEG_QUALITY: u8 = 92;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub appearance: theme::Appearance,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animations_enabled: Option<bool>,
    #[serde(skip_serializing_if = "typography::Preferences::is_default")]
    pub font: typography::Preferences,
    pub checkerboard_backdrop: bool,
    pub strip_privacy_on_copy: bool,
    pub default_jpeg_quality: u8,
    pub inspector_open: bool,
    pub filmstrip_open: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            appearance: theme::Appearance::default(),
            animations_enabled: None,
            font: typography::Preferences::default(),
            checkerboard_backdrop: true,
            strip_privacy_on_copy: false,
            default_jpeg_quality: DEFAULT_JPEG_QUALITY,
            inspector_open: true,
            filmstrip_open: true,
        }
    }
}
