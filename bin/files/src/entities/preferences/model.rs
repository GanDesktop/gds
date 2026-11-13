//! Persisted Files preferences and their editor schema.

use gds::{
    appearance::{theme, typography},
    i18n::Language,
    ui::time,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesPreferences {
    pub show_hidden: bool,
    pub auto_focus_search: bool,
    pub probe_budget: usize,
    pub sidebar: SidebarPreferences,
    /// Preferred content height above the terminal dock; shared by this window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_content_height: Option<f32>,
    /// Default browsing root, keyed by the volume's stable identity.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub volume_mount_points: std::collections::BTreeMap<String, std::path::PathBuf>,
}

impl Default for FilesPreferences {
    fn default() -> Self {
        Self {
            show_hidden: false,
            auto_focus_search: false,
            probe_budget: crate::entities::file::x::preferences::DEFAULT_PROBE_BUDGET,
            sidebar: SidebarPreferences::default(),
            terminal_content_height: None,
            volume_mount_points: std::collections::BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarPreferences {
    pub show_volumes: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tabs_height: Option<f32>,
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
    pub tags: crate::entities::tags::x::preferences::Preferences,
    pub terminal: gds::ui::terminal::Preferences,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_enabled: Option<bool>,
}

impl Preferences {
    pub fn terminal_enabled(&self) -> bool {
        self.terminal_enabled.unwrap_or(true)
    }
}
