//! Persisted Files preferences and their application to the current window.

use gds::{
    appearance::{theme, typography},
    i18n::Language,
    ui::time,
};
use gpui_kit::App;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesPreferences {
    pub show_hidden: bool,
    pub sidebar: SidebarPreferences,
    /// Preferred content height above the terminal dock; shared by this window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_content_height: Option<f32>,
    /// Default browsing root, keyed by the volume's stable identity.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub volume_mount_points: std::collections::BTreeMap<String, std::path::PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarPreferences {
    pub show_volumes: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tabs_height: Option<f32>,
    #[serde(alias = "devices_collapsed")]
    pub volumes_collapsed: bool,
    pub locations_collapsed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, gds::ConfigEditor)]
#[serde(default)]
pub struct Preferences {
    pub appearance: theme::Appearance,
    /// None enables interface animations while respecting system reduced motion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animations_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[setting(label = "form-language")]
    pub language: Option<Language>,
    /// How timestamps are displayed in this application.
    #[setting(label = "form-time-format")]
    pub time_format: time::Format,
    /// Font family and base text size for this application.
    #[serde(skip_serializing_if = "typography::Preferences::is_default")]
    pub font: typography::Preferences,
    pub files: FilesPreferences,
    pub terminal: gds::ui::terminal::Preferences,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_enabled: Option<bool>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error(transparent)]
    Theme(#[from] theme::Error),
    #[error(transparent)]
    Language(#[from] crate::shared::i18n::Error),
    #[error(transparent)]
    Typography(#[from] typography::Error),
}

impl Preferences {
    pub fn terminal_enabled(&self) -> bool {
        self.terminal_enabled.unwrap_or(true)
    }

    /// Apply shared presentation changes to an initialized application.
    /// Application-specific values are handled by the caller.
    ///
    /// # Errors
    /// Reports invalid theme/material/font values, unavailable palette data or
    /// invalid translations.
    pub fn apply_shared(&self, previous: &Self, cx: &mut App) -> Result<(), ApplyError> {
        let appearance = &self.appearance;
        let old_appearance = &previous.appearance;
        if appearance.source != old_appearance.source
            || appearance.mode != old_appearance.mode
            || (appearance.source == theme::Source::Builtin
                && appearance.builtin != old_appearance.builtin)
        {
            theme::apply(appearance, cx)?;
        } else if appearance != old_appearance {
            theme::update_opacity(appearance.content_opacity, appearance.sidebar_opacity, cx)?;
        }
        if self.font != previous.font {
            typography::install(&self.font, cx)?;
        }
        if self.language != previous.language {
            crate::shared::i18n::install(self.language.unwrap_or_else(Language::detect), cx)?;
        }
        if self.time_format != previous.time_format {
            time::apply(self.time_format, cx);
        }
        if self.animations_enabled != previous.animations_enabled {
            gds::appearance::motion::set_animations_enabled(
                self.animations_enabled.unwrap_or(true),
                cx,
            );
        }
        Ok(())
    }
}
