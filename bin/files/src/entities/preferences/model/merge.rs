//! Exhaustive three-way merge for concurrent preference saves.
//!
//! Each struct is exhaustively destructured so adding a persisted field forces
//! merge coverage at compile time.

use std::collections::BTreeMap;

use gds::appearance::{theme, typography};

use crate::entities::tags::x::preferences as tags;

use super::{
    EnrichmentPreferences, FilesPreferences, Preferences, PreviewPreferences, SidebarPreferences,
    StreamingPreferences, ThumbnailerCommands, ThumbnailerPreferences, TransferPreferences,
    WatchingPreferences,
};

macro_rules! merge_struct {
    ($ty:path { $($field:ident => $merge:path),* $(,)? }, $base:expr, $local:expr, $disk:expr) => {{
        let $ty { $($field: _),* } = $base;
        $ty {
            $(
                $field: $merge(&$base.$field, &$local.$field, &$disk.$field),
            )*
        }
    }};
}

fn field<T: Clone + PartialEq>(base: &T, local: &T, disk: &T) -> T {
    if local == base {
        disk.clone()
    } else {
        local.clone()
    }
}

fn map<K: Clone + Ord, V: Clone + PartialEq>(
    base: &BTreeMap<K, V>,
    local: &BTreeMap<K, V>,
    disk: &BTreeMap<K, V>,
) -> BTreeMap<K, V> {
    let mut merged = BTreeMap::new();
    for key in base.keys().chain(local.keys()).chain(disk.keys()) {
        if merged.contains_key(key) {
            continue;
        }
        if let Some(value) = field(&base.get(key), &local.get(key), &disk.get(key)) {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

fn appearance(
    base: &theme::Appearance,
    local: &theme::Appearance,
    disk: &theme::Appearance,
) -> theme::Appearance {
    merge_struct!(
        theme::Appearance {
            source => field,
            builtin => field,
            mode => field,
            content_opacity => field,
            sidebar_opacity => field,
            rounded_corners => field,
        },
        base,
        local,
        disk
    )
}

fn typography(
    base: &typography::Preferences,
    local: &typography::Preferences,
    disk: &typography::Preferences,
) -> typography::Preferences {
    merge_struct!(
        typography::Preferences {
            family => field,
            size => field,
        },
        base,
        local,
        disk
    )
}

fn tags_commands(
    base: &tags::Commands,
    local: &tags::Commands,
    disk: &tags::Commands,
) -> tags::Commands {
    merge_struct!(
        tags::Commands {
            backend => field,
            control => field,
            kio_client => field,
        },
        base,
        local,
        disk
    )
}

fn tags_prefs(
    base: &tags::Preferences,
    local: &tags::Preferences,
    disk: &tags::Preferences,
) -> tags::Preferences {
    merge_struct!(
        tags::Preferences {
            enabled => field,
            commands => tags_commands,
        },
        base,
        local,
        disk
    )
}

fn thumbnailer_commands(
    base: &ThumbnailerCommands,
    local: &ThumbnailerCommands,
    disk: &ThumbnailerCommands,
) -> ThumbnailerCommands {
    merge_struct!(
        ThumbnailerCommands {
            bwrap => field,
        },
        base,
        local,
        disk
    )
}

fn thumbnailers(
    base: &ThumbnailerPreferences,
    local: &ThumbnailerPreferences,
    disk: &ThumbnailerPreferences,
) -> ThumbnailerPreferences {
    merge_struct!(
        ThumbnailerPreferences {
            enabled => field,
            use_bwrap => field,
            commands => thumbnailer_commands,
        },
        base,
        local,
        disk
    )
}

fn terminal(
    base: &gds::ui::terminal::Preferences,
    local: &gds::ui::terminal::Preferences,
    disk: &gds::ui::terminal::Preferences,
) -> gds::ui::terminal::Preferences {
    merge_struct!(
        gds::ui::terminal::Preferences {
            shell => field,
            shell_args => field,
            login_shell => field,
            font_family => field,
            font_size => field,
            scrollback_lines => field,
            cursor_blink => field,
            alt_sends_escape => field,
        },
        base,
        local,
        disk
    )
}

fn streaming(
    base: &StreamingPreferences,
    local: &StreamingPreferences,
    disk: &StreamingPreferences,
) -> StreamingPreferences {
    merge_struct!(
        StreamingPreferences {
            initial_delay_ms => field,
            interval_ms => field,
            min_entries => field,
        },
        base,
        local,
        disk
    )
}

fn enrichment(
    base: &EnrichmentPreferences,
    local: &EnrichmentPreferences,
    disk: &EnrichmentPreferences,
) -> EnrichmentPreferences {
    merge_struct!(
        EnrichmentPreferences {
            content_probing => field,
            priority_limit => field,
            coalesce_ms => field,
            reorder_coalesce_ms => field,
            coalesce_max_updates => field,
            max_concurrent_batches => field,
            batch_size => field,
            child_count_limit => field,
        },
        base,
        local,
        disk
    )
}

fn preview(
    base: &PreviewPreferences,
    local: &PreviewPreferences,
    disk: &PreviewPreferences,
) -> PreviewPreferences {
    merge_struct!(
        PreviewPreferences {
            workers => field,
            cache_entries => field,
            cache_mib => field,
            max_thumbnail_source_mib => field,
            external_thumbnail_timeout_secs => field,
        },
        base,
        local,
        disk
    )
}

fn watching(
    base: &WatchingPreferences,
    local: &WatchingPreferences,
    disk: &WatchingPreferences,
) -> WatchingPreferences {
    merge_struct!(
        WatchingPreferences {
            debounce_ms => field,
            incremental_limit => field,
        },
        base,
        local,
        disk
    )
}

fn transfer(
    base: &TransferPreferences,
    local: &TransferPreferences,
    disk: &TransferPreferences,
) -> TransferPreferences {
    merge_struct!(
        TransferPreferences {
            local_buffer_kib => field,
            remote_buffer_kib => field,
        },
        base,
        local,
        disk
    )
}

fn sidebar(
    base: &SidebarPreferences,
    local: &SidebarPreferences,
    disk: &SidebarPreferences,
) -> SidebarPreferences {
    merge_struct!(
        SidebarPreferences {
            show_quick_access => field,
            show_volumes => field,
            tabs_height => field,
            quick_access_collapsed => field,
            volumes_collapsed => field,
            locations_collapsed => field,
        },
        base,
        local,
        disk
    )
}

fn files(
    base: &FilesPreferences,
    local: &FilesPreferences,
    disk: &FilesPreferences,
) -> FilesPreferences {
    merge_struct!(
        FilesPreferences {
            show_hidden => field,
            auto_focus_search => field,
            probe_budget => field,
            streaming => streaming,
            enrichment => enrichment,
            preview => preview,
            watching => watching,
            transfer => transfer,
            sidebar => sidebar,
            terminal_content_height => field,
            volume_mount_points => map,
        },
        base,
        local,
        disk
    )
}

impl Preferences {
    /// Three-way merge preserving untouched external edits on `disk` while
    /// applying explicit local changes relative to `base`.
    pub(in crate::entities::preferences) fn merge(base: &Self, local: &Self, disk: &Self) -> Self {
        merge_struct!(
            Self {
                appearance => appearance,
                animations_enabled => field,
                language => field,
                time_format => field,
                font => typography,
                files => files,
                tags => tags_prefs,
                thumbnailers => thumbnailers,
                terminal => terminal,
                terminal_enabled => field,
            },
            base,
            local,
            disk
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn merge_preserves_disjoint_external_fields_and_applies_local_edits() {
        let base = Preferences::default();

        let mut disk = base.clone();
        disk.files.show_hidden = true;
        disk.files.streaming.initial_delay_ms = 80;
        disk.files
            .volume_mount_points
            .insert("disk-vol".into(), PathBuf::from("/mnt/disk"));
        disk.terminal.font_size = 16.0;

        let mut local = base.clone();
        local.files.sidebar.show_volumes = true;
        local.files.streaming.min_entries = 250;
        local
            .files
            .volume_mount_points
            .insert("local-vol".into(), PathBuf::from("/mnt/local"));

        let merged = Preferences::merge(&base, &local, &disk);
        assert!(merged.files.show_hidden);
        assert!(merged.files.sidebar.show_volumes);
        assert_eq!(merged.files.streaming.initial_delay_ms, 80);
        assert_eq!(merged.files.streaming.min_entries, 250);
        assert_eq!(merged.terminal.font_size, 16.0);
        assert_eq!(
            merged.files.volume_mount_points.get("disk-vol"),
            Some(&PathBuf::from("/mnt/disk"))
        );
        assert_eq!(
            merged.files.volume_mount_points.get("local-vol"),
            Some(&PathBuf::from("/mnt/local"))
        );
    }

    #[test]
    fn merge_prefers_explicit_local_edit_on_same_field_conflict_and_respects_map_deletions() {
        let mut base = Preferences::default();
        base.files
            .volume_mount_points
            .insert("keep-remove-disk".into(), PathBuf::from("/mnt/a"));
        base.files
            .volume_mount_points
            .insert("remove-local".into(), PathBuf::from("/mnt/b"));

        let mut disk = base.clone();
        disk.files.sidebar.show_volumes = false;
        disk.appearance.mode = theme::ModePreference::Light;
        disk.files.volume_mount_points.remove("keep-remove-disk");

        let mut local = base.clone();
        local.files.sidebar.show_volumes = true;
        local.appearance.mode = theme::ModePreference::Dark;
        local.files.volume_mount_points.remove("remove-local");

        let merged = Preferences::merge(&base, &local, &disk);
        assert!(merged.files.sidebar.show_volumes);
        assert_eq!(merged.appearance.mode, theme::ModePreference::Dark);
        assert!(merged.files.volume_mount_points.is_empty());
    }
}
