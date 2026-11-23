use gds::i18n;
use gds::ui::button::{ButtonProps, ButtonSize, ButtonVariant};
use gds::ui::settings::SettingResetProps;
use gds::ui::{IconButtonProps, IconName, IconProps};
use zgui::prelude::*;
use zgui::view::install_stylesheet;
use zgui::{css, view};

use crate::entities::preferences::FilesPreferences;

const PERFORMANCE_SHEET: &str = "gds-files-settings-performance";
const PERFORMANCE_CSS: &str = css!(
    r#"
    .gds-settings-perf {
        display: flex;
        flex-direction: column;
        gap: 24px;
    }

    .gds-settings-perf__banner {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        align-items: flex-start;
        gap: 12px;
        padding: 16px;
        border-radius: 6px;
        border: 1px solid var(--gds-border);
        background: color-mix(in srgb, var(--gds-surface) 50%, transparent);
    }

    .gds-settings-perf__banner[data-locked="true"] {
        border-color: color-mix(in srgb, var(--gds-warning) 40%, transparent);
        background: color-mix(in srgb, var(--gds-warning) 8%, transparent);
    }

    .gds-settings-perf__icon {
        padding-top: 2px;
        color: var(--gds-muted-foreground);
    }

    .gds-settings-perf__banner[data-locked="true"] .gds-settings-perf__icon {
        color: var(--gds-warning);
    }

    .gds-settings-perf__text {
        display: flex;
        flex-direction: column;
        flex: 1 1 180px;
        min-width: 0;
        gap: 4px;
    }

    .gds-settings-perf__title {
        font-weight: 600;
    }

    .gds-settings-perf__desc {
        color: var(--gds-muted-foreground);
        white-space: normal;
    }

    .gds-settings-perf__action {
        display: flex;
        align-items: center;
        align-self: center;
        flex-shrink: 0;
        margin-left: auto;
    }
    "#
);

/// Form model for GDS Files runtime performance parameters displayed on [`super::Page::Performance`].
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, gds::ConfigEditor,
)]
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
        label = "files-advanced-content-probing",
        help = "files-advanced-content-probing-help",
        group = "files-advanced-group-enrichment"
    )]
    pub content_probing: bool,
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
            content_probing: files.enrichment.content_probing,
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
    #[cfg(test)]
    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(in crate::widgets::settings::ui) fn apply_to(&self, files: &mut FilesPreferences) -> bool {
        if *self == Self::from(&*files) {
            return false;
        }
        files.streaming.initial_delay_ms = self.stream_initial_delay_ms;
        files.streaming.interval_ms = self.stream_interval_ms;
        files.streaming.min_entries = self.stream_min_entries;
        files.enrichment.content_probing = self.content_probing;
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
    pub(in crate::widgets::settings::ui) fn performance(&self) -> AnyView {
        install_stylesheet(PERFORMANCE_SHEET, PERFORMANCE_CSS);
        let unlocked = self.performance_unlocked;
        let reset = self.clone();
        let focus = self.page_focus;
        let preferences = self.preferences;
        let disabled = Signal::derive_local(move || !unlocked.get());
        let editor = self.subform(
            |p| PerformanceForm::from(&p.files),
            move |p, v| {
                if unlocked.get_untracked() {
                    v.apply_to(&mut p.files);
                }
            },
        );
        let form_view = editor.disabled(disabled).view();

        AnyView::new(view! {
            column(class = "gds-settings-perf", attr:data-testid = "settings-performance-page") {
                row(
                    class = "gds-settings-perf__banner",
                    attr:data-locked = move || (!unlocked.get()).then(|| "true".to_owned()),
                    attr:data-testid = "settings-performance-notice",
                ) {
                    box(class = "gds-settings-perf__icon") {
                        {move || {
                            let name = if unlocked.get() {
                                IconName::Info
                            } else {
                                IconName::TriangleAlert
                            };
                            view! { Icon(name = name, size = 16.0) }.into_view()
                        }}
                    }
                    column(class = "gds-settings-perf__text") {
                        text(class = "gds-settings-perf__title") {{move || i18n::text("settings-performance-notice-title")}}
                        text(class = "gds-settings-perf__desc") {{move || i18n::text("settings-performance-notice")}}
                    }
                    box(class = "gds-settings-perf__action") {
                        {move || {
                            let reset_view = reset.clone();
                            if unlocked.get() {
                                let modified = preferences.with(|p| {
                                    PerformanceForm::from(&p.files) != PerformanceForm::default()
                                });
                                AnyView::new(view! {
                                    SettingReset {
                                        {modified.then(|| view! {
                                            IconButton(
                                                icon = IconName::RotateCcw,
                                                label = Signal::derive_local(|| i18n::text("form-reset")),
                                                attr:data-testid = "settings-performance-reset",
                                                on:click = move |_| {
                                                    reset_view.apply(|p| {
                                                        PerformanceForm::default().apply_to(&mut p.files);
                                                    });
                                                    focus.focus();
                                                },
                                            )
                                        }.into_view())}
                                    }
                                })
                            } else {
                                AnyView::new(view! {
                                    Button(
                                        variant = ButtonVariant::Default,
                                        size = ButtonSize::Sm,
                                        attr:data-testid = "settings-performance-unlock",
                                        on:click = move |_| {
                                            unlocked.set(true);
                                            focus.focus();
                                        },
                                    ) {{move || i18n::text("settings-performance-unlock")}}
                                })
                            }
                        }}
                    }
                }
                {form_view}
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn performance_form_projects_applies_and_resets_without_touching_basic_settings() {
        let mut source = FilesPreferences::default();
        assert!(PerformanceForm::from(&source).is_default());

        source.streaming.initial_delay_ms = 200;
        source.streaming.interval_ms = 400;
        source.streaming.min_entries = 350;
        source.probe_budget = 512;
        source.enrichment.content_probing = false;
        source.enrichment.max_concurrent_batches = 8;
        source.enrichment.batch_size = 64;
        source.enrichment.child_count_limit = 2500;
        source.enrichment.priority_limit = 256;
        source.enrichment.coalesce_ms = 64;
        source.enrichment.reorder_coalesce_ms = 300;
        source.enrichment.coalesce_max_updates = 1024;
        source.preview.workers = 4;
        source.preview.cache_entries = 512;
        source.preview.cache_mib = 256;
        source.preview.max_thumbnail_source_mib = 64;
        source.preview.video_thumbnail_timeout_secs = 20;
        source.watching.debounce_ms = 250;
        source.watching.incremental_limit = 512;
        source.transfer.local_buffer_kib = 128;
        source.transfer.remote_buffer_kib = 256;

        let custom_form = PerformanceForm::from(&source);
        assert!(!custom_form.is_default());

        let mut target = FilesPreferences {
            show_hidden: true,
            auto_focus_search: false,
            ..Default::default()
        };
        target.sidebar.show_volumes = false;
        target
            .volume_mount_points
            .insert("uuid-1".into(), PathBuf::from("/mnt/data"));

        assert!(custom_form.apply_to(&mut target));
        assert!(!custom_form.apply_to(&mut target));

        assert_eq!(target.streaming, source.streaming);
        assert_eq!(target.probe_budget, source.probe_budget);
        assert_eq!(target.enrichment, source.enrichment);
        assert_eq!(target.preview, source.preview);
        assert_eq!(target.watching, source.watching);
        assert_eq!(target.transfer, source.transfer);

        assert!(PerformanceForm::default().apply_to(&mut target));
        let defaults = FilesPreferences::default();
        assert_eq!(target.streaming, defaults.streaming);
        assert_eq!(target.probe_budget, defaults.probe_budget);
        assert_eq!(target.enrichment, defaults.enrichment);
        assert_eq!(target.preview, defaults.preview);
        assert_eq!(target.watching, defaults.watching);
        assert_eq!(target.transfer, defaults.transfer);
        assert!(target.show_hidden);
        assert!(!target.auto_focus_search);
        assert!(!target.sidebar.show_volumes);
        assert_eq!(
            target.volume_mount_points.get("uuid-1"),
            Some(&PathBuf::from("/mnt/data"))
        );
    }
}
