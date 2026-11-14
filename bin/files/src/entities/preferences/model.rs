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
    #[serde(
        default = "default_probe_budget",
        skip_serializing_if = "is_default_probe_budget"
    )]
    pub probe_budget: usize,
    #[serde(default, skip_serializing_if = "StreamingPreferences::is_default")]
    pub streaming: StreamingPreferences,
    #[serde(default, skip_serializing_if = "EnrichmentPreferences::is_default")]
    pub enrichment: EnrichmentPreferences,
    #[serde(default, skip_serializing_if = "PreviewPreferences::is_default")]
    pub preview: PreviewPreferences,
    #[serde(default, skip_serializing_if = "WatchingPreferences::is_default")]
    pub watching: WatchingPreferences,
    #[serde(default, skip_serializing_if = "TransferPreferences::is_default")]
    pub transfer: TransferPreferences,
    pub sidebar: SidebarPreferences,
    /// Preferred content height above the terminal dock; shared by this window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_content_height: Option<f32>,
    /// Default browsing root, keyed by the volume's stable identity.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub volume_mount_points: std::collections::BTreeMap<String, std::path::PathBuf>,
}

fn default_probe_budget() -> usize {
    crate::entities::file::x::preferences::DEFAULT_PROBE_BUDGET
}

fn is_default_probe_budget(value: &usize) -> bool {
    *value == default_probe_budget()
}

impl Default for FilesPreferences {
    fn default() -> Self {
        Self {
            show_hidden: false,
            auto_focus_search: false,
            probe_budget: default_probe_budget(),
            streaming: StreamingPreferences::default(),
            enrichment: EnrichmentPreferences::default(),
            preview: PreviewPreferences::default(),
            watching: WatchingPreferences::default(),
            transfer: TransferPreferences::default(),
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
    /// Number of entries probed per background enrichment batch.
    pub batch_size: usize,
    /// Maximum direct children counted per folder before displaying a lower bound.
    pub child_count_limit: u32,
}

impl Default for EnrichmentPreferences {
    fn default() -> Self {
        use crate::entities::file::x::preferences::{
            DEFAULT_BATCH_SIZE, DEFAULT_CHILD_COUNT_LIMIT, DEFAULT_COALESCE_MAX_UPDATES,
            DEFAULT_COALESCE_MS, DEFAULT_MAX_CONCURRENT_BATCHES, DEFAULT_PRIORITY_LIMIT,
            DEFAULT_REORDER_COALESCE_MS,
        };
        Self {
            priority_limit: DEFAULT_PRIORITY_LIMIT,
            coalesce_ms: DEFAULT_COALESCE_MS,
            reorder_coalesce_ms: DEFAULT_REORDER_COALESCE_MS,
            coalesce_max_updates: DEFAULT_COALESCE_MAX_UPDATES,
            max_concurrent_batches: DEFAULT_MAX_CONCURRENT_BATCHES,
            batch_size: DEFAULT_BATCH_SIZE,
            child_count_limit: DEFAULT_CHILD_COUNT_LIMIT,
        }
    }
}

impl EnrichmentPreferences {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Configurable worker concurrency, memory cache limits, and media decoding bounds for previews.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewPreferences {
    pub workers: usize,
    pub cache_entries: usize,
    pub cache_mib: usize,
    pub max_thumbnail_source_mib: u64,
    pub video_thumbnail_timeout_secs: u64,
}

impl Default for PreviewPreferences {
    fn default() -> Self {
        Self {
            workers: gds::ui::file_inspection::DEFAULT_WORKERS,
            cache_entries: gds::ui::file_inspection::DEFAULT_CACHE_ENTRIES,
            cache_mib: gds::ui::file_inspection::DEFAULT_CACHE_MIB,
            max_thumbnail_source_mib:
                gds::service::file_inspection::DEFAULT_MAX_THUMBNAIL_SOURCE_MIB,
            video_thumbnail_timeout_secs:
                gds::service::file_inspection::DEFAULT_VIDEO_THUMBNAIL_TIMEOUT_SECS,
        }
    }
}

impl PreviewPreferences {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn cache_options(&self) -> gds::ui::file_inspection::Options {
        gds::ui::file_inspection::Options {
            capacity: self.cache_entries.max(1),
            byte_budget: self.cache_mib.max(1).saturating_mul(1024 * 1024),
            workers: self.workers.max(1),
            inspection: gds::service::file_inspection::Options {
                max_thumbnail_source_bytes: self
                    .max_thumbnail_source_mib
                    .max(1)
                    .saturating_mul(1024 * 1024),
                video_thumbnail_timeout: std::time::Duration::from_secs(
                    self.video_thumbnail_timeout_secs.max(1),
                ),
            },
        }
    }
}

/// Configurable filesystem watch coalescing and incremental reconciliation limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchingPreferences {
    pub debounce_ms: u64,
    pub incremental_limit: usize,
}

impl Default for WatchingPreferences {
    fn default() -> Self {
        use crate::entities::file::x::preferences::{
            DEFAULT_WATCH_DEBOUNCE_MS, DEFAULT_WATCH_INCREMENTAL_LIMIT,
        };
        Self {
            debounce_ms: DEFAULT_WATCH_DEBOUNCE_MS,
            incremental_limit: DEFAULT_WATCH_INCREMENTAL_LIMIT,
        }
    }
}

impl WatchingPreferences {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn debounce(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.debounce_ms)
    }
}

/// Configurable I/O buffer sizes for local and remote file transfers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransferPreferences {
    pub local_buffer_kib: usize,
    pub remote_buffer_kib: usize,
}

impl Default for TransferPreferences {
    fn default() -> Self {
        Self {
            local_buffer_kib: Self::DEFAULT_LOCAL_BUFFER_KIB,
            remote_buffer_kib: Self::DEFAULT_REMOTE_BUFFER_KIB,
        }
    }
}

impl TransferPreferences {
    pub const DEFAULT_LOCAL_BUFFER_KIB: usize = 64;
    pub const DEFAULT_REMOTE_BUFFER_KIB: usize = 128;

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn local_buffer_bytes(&self) -> usize {
        self.local_buffer_kib.saturating_mul(1024).max(4096)
    }

    pub fn remote_buffer_bytes(&self) -> usize {
        self.remote_buffer_kib.saturating_mul(1024).max(4096)
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
    fn performance_preferences_skip_defaults_and_roundtrip_toml() {
        let defaults = Preferences::default();
        let serialized_default =
            toml::to_string_pretty(&defaults).expect("serialize default preferences");
        assert!(!serialized_default.contains("probe_budget"));
        assert!(!serialized_default.contains("[files.streaming]"));
        assert!(!serialized_default.contains("[files.enrichment]"));
        assert!(!serialized_default.contains("[files.preview]"));
        assert!(!serialized_default.contains("[files.watching]"));
        assert!(!serialized_default.contains("[files.transfer]"));

        let parsed_empty: Preferences = toml::from_str("").expect("deserialize empty TOML");
        assert_eq!(parsed_empty, defaults);

        let mut custom = Preferences::default();
        custom.files.probe_budget = 512;
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
            batch_size: 48,
            child_count_limit: 2500,
        };
        custom.files.preview = PreviewPreferences {
            workers: 4,
            cache_entries: 512,
            cache_mib: 256,
            max_thumbnail_source_mib: 64,
            video_thumbnail_timeout_secs: 20,
        };
        custom.files.watching = WatchingPreferences {
            debounce_ms: 300,
            incremental_limit: 512,
        };
        custom.files.transfer = TransferPreferences {
            local_buffer_kib: 128,
            remote_buffer_kib: 256,
        };
        let serialized = toml::to_string_pretty(&custom).expect("serialize custom preferences");
        assert!(serialized.contains("probe_budget = 512"));
        assert!(serialized.contains("[files.streaming]"));
        assert!(serialized.contains("[files.enrichment]"));
        assert!(serialized.contains("[files.preview]"));
        assert!(serialized.contains("[files.watching]"));
        assert!(serialized.contains("[files.transfer]"));

        let parsed: Preferences =
            toml::from_str(&serialized).expect("deserialize custom preferences");
        assert_eq!(parsed.files.probe_budget, custom.files.probe_budget);
        assert_eq!(parsed.files.streaming, custom.files.streaming);
        assert_eq!(parsed.files.enrichment, custom.files.enrichment);
        assert_eq!(parsed.files.preview, custom.files.preview);
        assert_eq!(parsed.files.watching, custom.files.watching);
        assert_eq!(parsed.files.transfer, custom.files.transfer);
    }
}
