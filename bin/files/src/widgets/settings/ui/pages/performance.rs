use crate::entities::preferences::FilesPreferences;

/// Form model for GDS Files runtime performance parameters displayed on [`super::Page::Performance`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, gds::ConfigEditor)]
pub(in crate::widgets::settings::ui) struct PerformanceForm {
    #[setting(
        label = "files-advanced-stream-delay",
        help = "files-advanced-stream-delay-help",
        group = "files-advanced-group-streaming",
        min = 25.0,
        max = 1000.0,
        step = 25.0,
        unit = " ms"
    )]
    pub stream_initial_delay_ms: u64,
    #[setting(
        label = "files-advanced-stream-interval",
        help = "files-advanced-stream-interval-help",
        group = "files-advanced-group-streaming",
        min = 50.0,
        max = 2000.0,
        step = 50.0,
        unit = " ms"
    )]
    pub stream_interval_ms: u64,
    #[setting(
        label = "files-advanced-stream-min-entries",
        help = "files-advanced-stream-min-entries-help",
        group = "files-advanced-group-streaming",
        min = 0.0,
        max = 2000.0,
        step = 50.0
    )]
    pub stream_min_entries: usize,

    #[setting(
        label = "probe-budget",
        help = "probe-budget-help",
        group = "files-advanced-group-enrichment",
        min = 0.0,
        max = 2048.0,
        step = 32.0
    )]
    pub probe_budget: usize,
    #[setting(
        label = "files-advanced-enrich-concurrency",
        help = "files-advanced-enrich-concurrency-help",
        group = "files-advanced-group-enrichment",
        min = 1.0,
        max = 16.0,
        step = 1.0
    )]
    pub enrich_max_concurrent_batches: usize,
    #[setting(
        label = "files-advanced-enrich-batch-size",
        help = "files-advanced-enrich-batch-size-help",
        group = "files-advanced-group-enrichment",
        min = 8.0,
        max = 256.0,
        step = 8.0
    )]
    pub enrich_batch_size: usize,
    #[setting(
        label = "files-advanced-child-count-limit",
        help = "files-advanced-child-count-limit-help",
        group = "files-advanced-group-enrichment",
        min = 100.0,
        max = 10000.0,
        step = 100.0
    )]
    pub child_count_limit: u32,
    #[setting(
        label = "files-advanced-enrich-priority-limit",
        help = "files-advanced-enrich-priority-limit-help",
        group = "files-advanced-group-enrichment",
        min = 32.0,
        max = 1024.0,
        step = 32.0
    )]
    pub enrich_priority_limit: usize,
    #[setting(
        label = "files-advanced-enrich-coalesce",
        help = "files-advanced-enrich-coalesce-help",
        group = "files-advanced-group-enrichment",
        min = 8.0,
        max = 250.0,
        step = 8.0,
        unit = " ms"
    )]
    pub enrich_coalesce_ms: u64,
    #[setting(
        label = "files-advanced-enrich-reorder-coalesce",
        help = "files-advanced-enrich-reorder-coalesce-help",
        group = "files-advanced-group-enrichment",
        min = 30.0,
        max = 1000.0,
        step = 10.0,
        unit = " ms"
    )]
    pub enrich_reorder_coalesce_ms: u64,
    #[setting(
        label = "files-advanced-enrich-max-updates",
        help = "files-advanced-enrich-max-updates-help",
        group = "files-advanced-group-enrichment",
        min = 64.0,
        max = 4096.0,
        step = 64.0
    )]
    pub enrich_coalesce_max_updates: usize,

    #[setting(
        label = "files-advanced-preview-workers",
        help = "files-advanced-preview-workers-help",
        group = "files-advanced-group-preview",
        min = 1.0,
        max = 8.0,
        step = 1.0
    )]
    pub preview_workers: usize,
    #[setting(
        label = "files-advanced-preview-cache-entries",
        help = "files-advanced-preview-cache-entries-help",
        group = "files-advanced-group-preview",
        min = 32.0,
        max = 2048.0,
        step = 32.0
    )]
    pub preview_cache_entries: usize,
    #[setting(
        label = "files-advanced-preview-cache-mib",
        help = "files-advanced-preview-cache-mib-help",
        group = "files-advanced-group-preview",
        min = 16.0,
        max = 1024.0,
        step = 16.0,
        unit = " MiB"
    )]
    pub preview_cache_mib: usize,
    #[setting(
        label = "files-advanced-thumbnail-source-mib",
        help = "files-advanced-thumbnail-source-mib-help",
        group = "files-advanced-group-preview",
        min = 4.0,
        max = 256.0,
        step = 4.0,
        unit = " MiB"
    )]
    pub max_thumbnail_source_mib: u64,
    #[setting(
        label = "files-advanced-video-timeout",
        help = "files-advanced-video-timeout-help",
        group = "files-advanced-group-preview",
        min = 2.0,
        max = 60.0,
        step = 2.0,
        unit = " s"
    )]
    pub video_thumbnail_timeout_secs: u64,

    #[setting(
        label = "files-advanced-watch-debounce",
        help = "files-advanced-watch-debounce-help",
        group = "files-advanced-group-io",
        min = 25.0,
        max = 1000.0,
        step = 25.0,
        unit = " ms"
    )]
    pub watch_debounce_ms: u64,
    #[setting(
        label = "files-advanced-watch-limit",
        help = "files-advanced-watch-limit-help",
        group = "files-advanced-group-io",
        min = 32.0,
        max = 2048.0,
        step = 32.0
    )]
    pub watch_incremental_limit: usize,
    #[setting(
        label = "files-advanced-local-buffer",
        help = "files-advanced-local-buffer-help",
        group = "files-advanced-group-io",
        min = 16.0,
        max = 1024.0,
        step = 16.0,
        unit = " KiB"
    )]
    pub local_copy_buffer_kib: usize,
    #[setting(
        label = "files-advanced-remote-buffer",
        help = "files-advanced-remote-buffer-help",
        group = "files-advanced-group-io",
        min = 16.0,
        max = 1024.0,
        step = 16.0,
        unit = " KiB"
    )]
    pub remote_transfer_buffer_kib: usize,
}

impl From<&FilesPreferences> for PerformanceForm {
    fn from(files: &FilesPreferences) -> Self {
        Self {
            stream_initial_delay_ms: files.streaming.initial_delay_ms,
            stream_interval_ms: files.streaming.interval_ms,
            stream_min_entries: files.streaming.min_entries,
            probe_budget: files.probe_budget,
            enrich_max_concurrent_batches: files.enrichment.max_concurrent_batches,
            enrich_batch_size: files.enrichment.batch_size,
            child_count_limit: files.enrichment.child_count_limit,
            enrich_priority_limit: files.enrichment.priority_limit,
            enrich_coalesce_ms: files.enrichment.coalesce_ms,
            enrich_reorder_coalesce_ms: files.enrichment.reorder_coalesce_ms,
            enrich_coalesce_max_updates: files.enrichment.coalesce_max_updates,
            preview_workers: files.preview.workers,
            preview_cache_entries: files.preview.cache_entries,
            preview_cache_mib: files.preview.cache_mib,
            max_thumbnail_source_mib: files.preview.max_thumbnail_source_mib,
            video_thumbnail_timeout_secs: files.preview.video_thumbnail_timeout_secs,
            watch_debounce_ms: files.watching.debounce_ms,
            watch_incremental_limit: files.watching.incremental_limit,
            local_copy_buffer_kib: files.transfer.local_buffer_kib,
            remote_transfer_buffer_kib: files.transfer.remote_buffer_kib,
        }
    }
}

impl Default for PerformanceForm {
    fn default() -> Self {
        Self::from(&FilesPreferences::default())
    }
}

impl PerformanceForm {
    pub(in crate::widgets::settings::ui) fn apply_to(&self, files: &mut FilesPreferences) -> bool {
        if *self == Self::from(&*files) {
            return false;
        }
        files.streaming.initial_delay_ms = self.stream_initial_delay_ms;
        files.streaming.interval_ms = self.stream_interval_ms;
        files.streaming.min_entries = self.stream_min_entries;
        files.probe_budget = self.probe_budget;
        files.enrichment.max_concurrent_batches = self.enrich_max_concurrent_batches;
        files.enrichment.batch_size = self.enrich_batch_size;
        files.enrichment.child_count_limit = self.child_count_limit;
        files.enrichment.priority_limit = self.enrich_priority_limit;
        files.enrichment.coalesce_ms = self.enrich_coalesce_ms;
        files.enrichment.reorder_coalesce_ms = self.enrich_reorder_coalesce_ms;
        files.enrichment.coalesce_max_updates = self.enrich_coalesce_max_updates;
        files.preview.workers = self.preview_workers;
        files.preview.cache_entries = self.preview_cache_entries;
        files.preview.cache_mib = self.preview_cache_mib;
        files.preview.max_thumbnail_source_mib = self.max_thumbnail_source_mib;
        files.preview.video_thumbnail_timeout_secs = self.video_thumbnail_timeout_secs;
        files.watching.debounce_ms = self.watch_debounce_ms;
        files.watching.incremental_limit = self.watch_incremental_limit;
        files.transfer.local_buffer_kib = self.local_copy_buffer_kib;
        files.transfer.remote_buffer_kib = self.remote_transfer_buffer_kib;
        true
    }
}

impl super::super::View {
    pub(in crate::widgets::settings::ui) fn performance(&self) -> anti_webui::prelude::Div {
        use anti_webui::prelude::*;
        use gds::{
            i18n,
            ui::{Icon, IconButton, settings::SettingsReset},
        };
        use gds_uim::el;

        let unlocked = self.performance_unlocked;
        let editor = self.clone();
        let reset = self.clone();
        let focus = self.page_focus;
        let preferences = self.preferences;
        el! { .div.{flex; flex_col}.[
            .div.{
                flex; items_start; gap: 12.0_f32; p: 16.0_f32; mb: 32.0_f32; rounded: 6.0_f32; border: 1.0_f32;
                dyn_style: move s => ${
                    if unlocked.get() {
                        s.bg(theme().surface.with_alpha(0.5)).border_color(theme().border)
                    } else {
                        s.bg(theme().warning.with_alpha(0.08)).border_color(theme().warning.with_alpha(0.4))
                    }
                };
            }.[
                .div.{pt: 2.0_f32;
                    dyn_style: move s => $(s.text_color(if unlocked.get() { theme().text_muted } else { theme().warning }));
                }.[(move () => $(if unlocked.get() { Icon::Info.view(16.0) } else { Icon::Warning.view(16.0) })).dyn_view],
                .div.{flex_1; min_w: 0.0_f32; flex; flex_col; gap: 4.0_f32}.[
                    (() => "settings-performance-notice-title".i18n::text).text.{font_weight: FontWeight::SEMIBOLD},
                    (() => "settings-performance-notice".i18n::text).text.{dyn_style: s => $(s.text_color(theme().text_muted))},
                ],
                (move () => ${
                    let reset = reset.clone();
                    el! { .div.{flex_none}.[
                        if unlocked.{get} [
                            .SettingsReset::default.[
                                (move () => $(preferences.with(|p| PerformanceForm::from(&p.files) != PerformanceForm::default())),
                                move () => ${
                                    let reset = reset.clone();
                                    el! { "settings-performance-reset".IconButton::new.{
                                        icon: Icon::Reset; label_fn: () => "form-reset".i18n::text;
                                        on_click: move _ => ${
                                            reset.apply(|p| { PerformanceForm::default().apply_to(&mut p.files); });
                                            focus.focus();
                                        };
                                    } }
                                }).show,
                            ],
                        ] else [
                            (() => "settings-performance-unlock".i18n::text).primary_button.{
                                h: 24.0_f32; px: 8.0_f32; py: 0.0_f32; font_weight: FontWeight::NORMAL;
                                on_click: move _ => ${
                                    unlocked.set(true);
                                    focus.focus();
                                };
                            },
                        ],
                    ] }
                }).dyn_view,
            ],
            (move () => ${
                let disabled = !unlocked.get();
                untrack(|| {
                    editor.subform(|p| PerformanceForm::from(&p.files), |p, v| {
                        v.apply_to(&mut p.files);
                    }).disabled(disabled)
                })
            }).dyn_view,
        ] }
    }
}
