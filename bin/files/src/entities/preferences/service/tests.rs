use super::*;
use crate::shared::app::FilesExt as _;
use gds::appearance::theme;
use gpui_kit::TestAppContext;
use std::sync::atomic::{AtomicBool, Ordering};

fn initialize(path: PathBuf, cx: &mut TestAppContext) -> Entity<Service> {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds().tasks().init_test(false);
        cx.gds().appearance().init();
        let mut preferences = Preferences::default();
        preferences.appearance.source = theme::Source::Builtin;
        cx.gds()
            .appearance()
            .theme()
            .apply(&preferences.appearance)
            .expect("initial theme");
        cx.gds()
            .appearance()
            .theme()
            .init_hot_reload(preferences.appearance.clone());
        cx.gds().files().preferences().install(preferences, path)
    })
}

#[gpui_kit::test]
async fn independent_edits_are_preserved_and_exit_drains_the_latest_snapshot(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().expect("configuration directory");
    let path = directory.path().join("settings.toml");
    let _service = initialize(path.clone(), cx);
    let (scope, expected) = cx.update(|cx| {
        cx.gds()
            .files()
            .preferences()
            .edit(|preferences| {
                preferences.files.show_hidden = true;
            })
            .expect("first window change");
        cx.gds()
            .files()
            .preferences()
            .edit(|preferences| {
                preferences.files.sidebar.tabs_height = Some(240.);
            })
            .expect("second window change");
        let expected = cx.gds_ro().files().preferences().current();
        assert!(expected.files.show_hidden);
        assert_eq!(expected.files.sidebar.tabs_height, Some(240.));
        (cx.gds_ro().tasks().scope(), expected)
    });
    scope.shutdown().await;
    let saved: Preferences =
        toml::from_str(&std::fs::read_to_string(path).expect("saved preferences"))
            .expect("valid preferences");
    assert_eq!(saved, expected);
    cx.update(|cx| {
        assert!(
            cx.gds()
                .files()
                .preferences()
                .edit(|preferences| preferences.files.show_hidden = false)
                .is_err()
        );
        assert_eq!(cx.gds_ro().files().preferences().current(), expected);
    });
}

#[gpui_kit::test]
async fn failed_persistence_reports_an_error_and_keeps_the_live_snapshot(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("configuration directory");
    let parent = directory.path().join("not-a-directory");
    std::fs::write(&parent, "keep this file").expect("blocked destination");
    let service = initialize(parent.join("settings.toml"), cx);
    let reported = Arc::new(AtomicBool::new(false));
    let notification = reported.clone();
    let scope = cx.update(|cx| {
        cx.subscribe(&service, move |_, _: &SaveFailed, _| {
            notification.store(true, Ordering::SeqCst);
        })
        .detach();
        cx.gds()
            .files()
            .preferences()
            .edit(|preferences| preferences.files.show_hidden = true)
            .expect("live change accepted");
        assert!(
            cx.gds_ro()
                .files()
                .preferences()
                .current()
                .files
                .show_hidden
        );
        cx.gds_ro().tasks().scope()
    });
    scope.shutdown().await;
    cx.run_until_parked();
    assert!(reported.load(Ordering::SeqCst));
    assert!(cx.update(|cx| {
        cx.gds_ro()
            .files()
            .preferences()
            .current()
            .files
            .show_hidden
    }));
    assert_eq!(
        std::fs::read_to_string(parent).expect("original file"),
        "keep this file"
    );
}

#[gpui_kit::test]
async fn invalid_font_rejects_the_entire_update_before_changing_the_theme(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("configuration directory");
    let path = directory.path().join("settings.toml");
    let _service = initialize(path.clone(), cx);
    let scope = cx.update(|cx| {
        let previous = cx.gds_ro().files().preferences().current();
        let palette = cx.gds_ro().appearance().theme().colors();
        let mut invalid = previous.clone();
        invalid.appearance.mode = theme::ModePreference::Light;
        invalid.font.size = f32::NAN;
        assert!(cx.gds().files().preferences().update(invalid).is_err());
        assert_eq!(cx.gds_ro().files().preferences().current(), previous);
        assert_eq!(cx.gds_ro().appearance().theme().colors(), palette);
        cx.gds_ro().tasks().scope()
    });
    scope.shutdown().await;
    assert!(!path.exists());
}
