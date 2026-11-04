//! Shared appearance data and validation, independent of the desktop runtime.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    #[serde(with = "language")]
    pub language: Language,
    pub ui_font_family: Option<String>,
    pub mono_font_family: Option<String>,
    pub font_fallbacks: Option<Vec<String>>,
    pub ui_font_size: Option<f32>,
    pub content_font_size: Option<f32>,
    pub content_line_height: Option<f32>,
    pub content_opacity: Option<f32>,
    pub sidebar_opacity: Option<f32>,
    pub rounded_corners: bool,
    /// None enables interface animations while respecting system reduced motion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animations_enabled: Option<bool>,
    pub composer_enter_behavior: ComposerEnterBehavior,
    /// None starts new conversations with the most recently used Agent.
    pub default_agent: Option<String>,
    /// None inherits the safe default: ask before deleting a conversation.
    pub confirm_session_delete: Option<bool>,
    /// None groups consecutive thoughts and tool calls into a collapsed activity block.
    pub auto_collapse_activity: Option<bool>,
    pub background: Background,
    pub theme_source: ThemeSource,
    pub builtin_theme: gds::appearance::theme::BuiltinTheme,
    pub theme_mode: ThemeMode,
    pub theme_file: Option<PathBuf>,
}

pub use gds::i18n::Language;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerEnterBehavior {
    #[default]
    Send,
    Newline,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    #[default]
    Auto,
    Opaque,
    Transparent,
    Blurred,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSource {
    #[default]
    Desktop,
    Builtin,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Desktop,
    System,
    Light,
    Dark,
}
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("{0} must be finite and greater than zero")]
    Positive(&'static str),
    #[error("{0} must be finite and between 0 and 1")]
    Opacity(&'static str),
    #[error("{0} must not contain an empty font name")]
    Font(&'static str),
    #[error("theme_file must not be empty")]
    ThemeFile,
}
impl Appearance {
    /// # Errors
    /// Rejects invalid sizes, opacity, font names, and empty theme paths.
    pub fn validate(&self) -> std::result::Result<(), ValidationError> {
        for (name, value) in [
            ("ui_font_size", self.ui_font_size),
            ("content_font_size", self.content_font_size),
            ("content_line_height", self.content_line_height),
        ] {
            if let Some(value) = value {
                positive(name, value)?;
            }
        }
        for (name, value) in [
            ("content_opacity", self.content_opacity),
            ("sidebar_opacity", self.sidebar_opacity),
        ] {
            if value.is_some_and(|v| !v.is_finite() || !(0. ..=1.).contains(&v)) {
                return Err(ValidationError::Opacity(name));
            }
        }
        for (name, value) in [
            ("ui_font_family", self.ui_font_family.as_deref()),
            ("mono_font_family", self.mono_font_family.as_deref()),
        ] {
            if value.is_some_and(|v| v.trim().is_empty()) {
                return Err(ValidationError::Font(name));
            }
        }
        if self
            .font_fallbacks
            .as_ref()
            .is_some_and(|v| v.iter().any(|v| v.trim().is_empty()))
        {
            return Err(ValidationError::Font("font_fallbacks"));
        }
        if self
            .theme_file
            .as_ref()
            .is_some_and(|v| v.as_os_str().is_empty())
        {
            return Err(ValidationError::ThemeFile);
        }
        Ok(())
    }
    /// # Errors
    /// Reports invalid JSON or preference values.
    #[cfg(test)]
    pub fn from_json(text: &str) -> anyhow::Result<Self> {
        use anyhow::Context as _;
        let preferences: Self =
            serde_json::from_str(text).context("invalid Agents appearance JSON")?;
        preferences.validate()?;
        Ok(preferences)
    }
}
/// # Errors
/// Rejects zero, negative, NaN, and infinite sizes, including desktop imports.
pub fn positive(name: &'static str, value: f32) -> Result<(), ValidationError> {
    if value.is_finite() && value > 0. {
        Ok(())
    } else {
        Err(ValidationError::Positive(name))
    }
}

mod language {
    use super::Language;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(language: &Language, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(language.locale())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Language, D::Error> {
        Language::deserialize(deserializer)
    }
}
