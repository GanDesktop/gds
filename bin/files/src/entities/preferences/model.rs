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
    #[serde(default, skip_serializing_if = "StreamingPreferences::is_default")]
    pub streaming: StreamingPreferences,
    #[serde(default, skip_serializing_if = "EnrichmentPreferences::is_default")]
    pub enrichment: EnrichmentPreferences,
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
            streaming: StreamingPreferences::default(),
            enrichment: EnrichmentPreferences::default(),
            sidebar: SidebarPreferences::default(),
            terminal_content_height: None,
            volume_mount_points: std::collections::BTreeMap::new(),
        }
    }
}

/// Configurable time and entry-count gates for progressive directory snapshots on slow storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamingPreferences {
    /// Elapsed milliseconds before emitting the first progressive snapshot.
    pub initial_delay_ms: u64,
    /// Minimum milliseconds between subsequent progressive snapshots during a slow scan.
    pub interval_ms: u64,
    /// Minimum collected entries before emitting progressive snapshots (`0` disables streaming).
    pub min_entries: usize,
}

impl Default for StreamingPreferences {
    fn default() -> Self {
        use crate::entities::file::x::preferences::{
            DEFAULT_STREAM_INITIAL_DELAY_MS, DEFAULT_STREAM_INTERVAL_MS, DEFAULT_STREAM_MIN_ENTRIES,
        };
        Self {
            initial_delay_ms: DEFAULT_STREAM_INITIAL_DELAY_MS,
            interval_ms: DEFAULT_STREAM_INTERVAL_MS,
            min_entries: DEFAULT_STREAM_MIN_ENTRIES,
        }
    }
}

impl StreamingPreferences {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn options(&self) -> crate::entities::file::x::preferences::StreamOptions {
        crate::entities::file::x::preferences::StreamOptions {
            initial_delay: std::time::Duration::from_millis(self.initial_delay_ms),
            interval: std::time::Duration::from_millis(self.interval_ms),
            min_entries: self.min_entries,
        }
    }
}

/// Configurable batch coalescing, concurrency, and priority-queue bounds for background enrichment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnrichmentPreferences {
    /// Maximum viewport-priority entries retained during rapid scrolling before shedding older rows.
    pub priority_limit: usize,
    /// Batch flush coalescing window (ms) when secondary metadata cannot alter row order.
    pub coalesce_ms: u64,
    /// Batch flush coalescing window (ms) when secondary metadata can alter order or filter membership.
    pub reorder_coalesce_ms: u64,
    /// Maximum pending updates buffered before forcing an immediate UI flush.
    pub coalesce_max_updates: usize,
    /// Maximum concurrent worker batches per directory enrichment pass.
    pub max_concurrent_batches: usize,
}

impl Default for EnrichmentPreferences {
    fn default() -> Self {
        use crate::entities::file::x::preferences::{
            DEFAULT_COALESCE_MAX_UPDATES, DEFAULT_COALESCE_MS, DEFAULT_MAX_CONCURRENT_BATCHES,
            DEFAULT_PRIORITY_LIMIT, DEFAULT_REORDER_COALESCE_MS,
        };
        Self {
            priority_limit: DEFAULT_PRIORITY_LIMIT,
            coalesce_ms: DEFAULT_COALESCE_MS,
            reorder_coalesce_ms: DEFAULT_REORDER_COALESCE_MS,
            coalesce_max_updates: DEFAULT_COALESCE_MAX_UPDATES,
            max_concurrent_batches: DEFAULT_MAX_CONCURRENT_BATCHES,
        }
    }
}

impl EnrichmentPreferences {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_and_enrichment_preferences_skip_defaults_and_roundtrip_toml() {
        let defaults = Preferences::default();
        let serialized_default =
            toml::to_string_pretty(&defaults).expect("serialize default preferences");
        assert!(!serialized_default.contains("[files.streaming]"));
        assert!(!serialized_default.contains("[files.enrichment]"));

        let mut custom = Preferences::default();
        custom.files.streaming = StreamingPreferences {
            initial_delay_ms: 80,
            interval_ms: 200,
            min_entries: 150,
        };
        custom.files.enrichment = EnrichmentPreferences {
            priority_limit: 96,
            coalesce_ms: 24,
            reorder_coalesce_ms: 120,
            coalesce_max_updates: 256,
            max_concurrent_batches: 6,
        };
        let serialized = toml::to_string_pretty(&custom).expect("serialize custom preferences");
        assert!(serialized.contains("[files.streaming]"));
        assert!(serialized.contains("[files.enrichment]"));

        let parsed: Preferences =
            toml::from_str(&serialized).expect("deserialize custom preferences");
        assert_eq!(parsed.files.streaming, custom.files.streaming);
        assert_eq!(parsed.files.enrichment, custom.files.enrichment);
    }
}
