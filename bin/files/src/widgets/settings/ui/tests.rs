use super::controls::Field;
use crate::entities::preferences::Preferences;
use gds::appearance::tokens::SurfaceOpacity;

#[test]
fn boolean_fields_detect_modifications_and_reset_to_defaults() {
    let mut preferences = Preferences::default();

    assert!(!Field::RoundedCorners.is_modified(&preferences));
    assert_eq!(Field::RoundedCorners.bool(&preferences), Some(false));
    assert!(Field::RoundedCorners.set_bool(&mut preferences, true));
    assert!(Field::RoundedCorners.is_modified(&preferences));
    Field::RoundedCorners.reset(&mut preferences);
    assert!(!Field::RoundedCorners.is_modified(&preferences));

    assert!(!Field::Animations.is_modified(&preferences));
    assert_eq!(Field::Animations.bool(&preferences), Some(true));
    assert!(Field::Animations.set_bool(&mut preferences, false));
    assert!(Field::Animations.is_modified(&preferences));
    Field::Animations.reset(&mut preferences);
    assert!(!Field::Animations.is_modified(&preferences));

    assert!(!Field::TerminalEnabled.is_modified(&preferences));
    assert_eq!(Field::TerminalEnabled.bool(&preferences), Some(true));
    assert!(Field::TerminalEnabled.set_bool(&mut preferences, false));
    assert!(Field::TerminalEnabled.is_modified(&preferences));
    Field::TerminalEnabled.reset(&mut preferences);
    assert!(!Field::TerminalEnabled.is_modified(&preferences));

    assert!(!Field::TagsEnabled.is_modified(&preferences));
    assert_eq!(Field::TagsEnabled.bool(&preferences), Some(false));
    assert!(Field::TagsEnabled.set_bool(&mut preferences, true));
    assert!(Field::TagsEnabled.is_modified(&preferences));
    Field::TagsEnabled.reset(&mut preferences);
    assert!(!Field::TagsEnabled.is_modified(&preferences));
}

#[test]
fn numeric_fields_detect_modifications_and_reset_to_defaults() {
    let mut preferences = Preferences::default();
    let defaults = SurfaceOpacity::default();

    assert!(!Field::ContentOpacity.is_modified(&preferences));
    assert_eq!(
        Field::ContentOpacity.number(&preferences),
        Some(defaults.content * 100.0)
    );
    Field::ContentOpacity.set_number(&mut preferences, 50.0);
    assert!(Field::ContentOpacity.is_modified(&preferences));
    Field::ContentOpacity.reset(&mut preferences);
    assert!(!Field::ContentOpacity.is_modified(&preferences));

    assert!(!Field::SidebarOpacity.is_modified(&preferences));
    assert_eq!(
        Field::SidebarOpacity.number(&preferences),
        Some(defaults.sidebar * 100.0)
    );
    Field::SidebarOpacity.set_number(&mut preferences, 40.0);
    assert!(Field::SidebarOpacity.is_modified(&preferences));
    Field::SidebarOpacity.reset(&mut preferences);
    assert!(!Field::SidebarOpacity.is_modified(&preferences));
}

#[test]
fn performance_form_projects_applies_and_resets_without_touching_basic_settings() {
    use super::pages::PerformanceForm;
    use crate::entities::preferences::FilesPreferences;
    use std::path::PathBuf;

    let mut source = FilesPreferences::default();
    assert!(PerformanceForm::from(&source).is_default());

    source.streaming.initial_delay_ms = 200;
    source.streaming.interval_ms = 400;
    source.streaming.min_entries = 350;
    source.probe_budget = 512;
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

    let mut target = FilesPreferences::default();
    target.show_hidden = true;
    target.auto_focus_search = false;
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

#[cfg(feature = "gui-tests")]
#[gpui_kit::test]
async fn performance_page_is_separate_last_section(cx: &mut gpui_kit::TestAppContext) {
    use gds::app::AppExt as _;
    use gds::appearance::theme;
    use gpui_kit::{AppContext, px, size, test::TestWindowExt};

    assert_eq!(super::Page::ALL.last(), Some(&super::Page::Performance));

    cx.executor().allow_parking();
    let tasks = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut().tasks().init_test(false);
        let catalog = crate::shared::i18n::prepare(gds::i18n::Language::English)
            .expect("prepare English translations");
        cx.gds_mut().i18n().install_catalog(catalog);
        cx.gds_mut().appearance().init();
        let mut preferences = Preferences::default();
        preferences.appearance.source = theme::Source::Builtin;
        cx.gds_mut()
            .appearance()
            .theme()
            .apply(&preferences.appearance)
            .expect("initial theme");
        cx.gds().tasks().scope()
    });

    let mut view_entity = None;
    let window = cx.open_window(size(px(900.), px(700.)), |window, cx| {
        let mut preferences = Preferences::default();
        preferences.appearance.source = theme::Source::Builtin;
        preferences.files.streaming.initial_delay_ms = 250;
        let view = cx.new(|cx| {
            let mut view =
                super::View::new(preferences, Default::default(), tasks.clone(), window, cx);
            view.page = super::Page::Files;
            view
        });
        view_entity = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    let view = view_entity.expect("settings view created");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find("config-field-stream_initial_delay_ms")
                .is_none()
        );

        window.click(("settings-page", super::Page::Performance.index()), cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).page, super::Page::Performance);
        assert!(window.find("settings-performance-page").visible());
        assert!(window.find("settings-performance-notice").visible());
        assert!(window.find("settings-performance-unlock").visible());
        assert!(!view.read(cx).performance_unlocked);
        assert!(view.read(cx).performance.read(cx).is_disabled());
        assert!(window.try_find("settings-performance-reset").is_none());
        assert!(
            window
                .try_find("config-field-stream_initial_delay_ms")
                .is_some()
        );

        window.click("config-field-stream_initial_delay_ms", cx);
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).preferences.files.streaming.initial_delay_ms,
            250
        );

        window.click("settings-performance-unlock", cx);
        window.render_frame(cx);
        assert!(view.read(cx).performance_unlocked);
        assert!(!view.read(cx).performance.read(cx).is_disabled());
        assert!(window.try_find("settings-performance-unlock").is_none());
        assert!(window.try_find("settings-performance-reset").is_some());

        window.click("settings-performance-reset", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).preferences.files.streaming.initial_delay_ms,
            Preferences::default().files.streaming.initial_delay_ms
        );
    })
    .expect("performance page renders cleanly and stays locked until confirmed");
}
