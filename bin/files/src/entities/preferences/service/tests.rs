use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use gds::{appearance::theme, service::task_manager::Supervisor};
use zgui::reactive::{Mounted, RenderEffect, flush, install, on_cleanup_local};

use super::*;

struct Harness {
    root: Option<Mounted>,
    service: Service,
    supervisor: Supervisor,
}

impl Harness {
    fn new(path: PathBuf) -> Self {
        let _ = install();
        let supervisor = Supervisor::new(false).expect("preference test runtime");
        let scope = supervisor.scope();
        let root = Mounted::new();
        let service = root.with(|| {
            let app = Gds::install(scope.clone());
            let mut preferences = Preferences::default();
            preferences.appearance.source = theme::Source::Builtin;
            app.appearance()
                .theme()
                .apply(&preferences.appearance)
                .expect("initial theme");
            app.appearance()
                .theme()
                .configure_hot_reload(preferences.appearance.clone());
            Service::new(preferences, path, &scope)
        });
        Self {
            root: Some(root),
            service,
            supervisor,
        }
    }

    fn with<R>(&self, run: impl FnOnce(&Service) -> R) -> R {
        self.root
            .as_ref()
            .expect("mounted harness")
            .with(|| Gds::current().with(|| run(&self.service)))
    }

    fn shutdown(&self) {
        self.supervisor.shutdown();
        for _ in 0..4 {
            flush();
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(root) = self.root.take() {
            root.unmount();
        }
    }
}

#[test]
fn independent_edits_are_preserved_and_exit_drains_the_latest_snapshot() {
    let directory = tempfile::tempdir().expect("configuration directory");
    let path = directory.path().join("settings.toml");
    let harness = Harness::new(path.clone());
    let expected = harness.with(|service| {
        service
            .edit(|preferences| preferences.files.show_hidden = true)
            .expect("first window change");
        service
            .edit(|preferences| preferences.files.sidebar.tabs_height = Some(240.0))
            .expect("second window change");
        let expected = service.snapshot();
        assert!(expected.files.show_hidden);
        assert_eq!(expected.files.sidebar.tabs_height, Some(240.0));
        expected
    });
    harness.shutdown();
    let saved: Preferences =
        toml::from_str(&std::fs::read_to_string(path).expect("saved preferences"))
            .expect("valid preferences");
    assert_eq!(saved, expected);
    harness.with(|service| {
        assert!(
            service
                .edit(|preferences| preferences.files.show_hidden = false)
                .is_err()
        );
        assert_eq!(service.snapshot(), expected);
    });
}

#[test]
fn failed_persistence_reports_an_error_and_keeps_the_live_snapshot() {
    let directory = tempfile::tempdir().expect("configuration directory");
    let parent = directory.path().join("not-a-directory");
    std::fs::write(&parent, "keep this file").expect("blocked destination");
    let harness = Harness::new(parent.join("settings.toml"));
    let reported = Arc::new(AtomicBool::new(false));
    let notification = reported.clone();
    harness.with(|service| {
        let error = service.error();
        let fx = RenderEffect::new(move |_| {
            if error.get().is_some() {
                notification.store(true, Ordering::SeqCst);
            }
        });
        on_cleanup_local(move || drop(fx));
        service
            .edit(|preferences| preferences.files.show_hidden = true)
            .expect("live change accepted");
        assert!(service.snapshot().files.show_hidden);
    });
    harness.shutdown();
    assert!(reported.load(Ordering::SeqCst));
    assert!(harness.with(|service| service.snapshot().files.show_hidden));
    assert_eq!(
        std::fs::read_to_string(parent).expect("original file"),
        "keep this file"
    );
}

#[test]
fn invalid_font_rejects_the_entire_update_before_changing_the_theme() {
    let directory = tempfile::tempdir().expect("configuration directory");
    let path = directory.path().join("settings.toml");
    let harness = Harness::new(path.clone());
    harness.with(|service| {
        let previous = service.snapshot();
        let palette = Gds::current().appearance().theme().colors();
        let mut invalid = previous.clone();
        invalid.appearance.mode = theme::ModePreference::Light;
        invalid.font.size = f32::NAN;
        assert!(service.update(invalid).is_err());
        assert_eq!(service.snapshot(), previous);
        assert_eq!(Gds::current().appearance().theme().colors(), palette);
    });
    harness.shutdown();
    assert!(!path.exists());
}

#[test]
fn external_config_edit_does_not_block_persisting_sidebar_show_volumes() {
    let directory = tempfile::tempdir().expect("configuration directory");
    let path = directory.path().join("settings.toml");

    let mut initial = Preferences::default();
    initial.appearance.source = theme::Source::Builtin;
    std::fs::write(
        &path,
        toml::to_string_pretty(&initial).expect("serialize initial preferences"),
    )
    .expect("seed settings.toml");

    let harness = Harness::new(path.clone());

    // Another process (for example a portal file picker) writes settings.toml externally.
    let mut external = initial;
    external.files.show_hidden = true;
    std::fs::write(
        &path,
        toml::to_string_pretty(&external).expect("serialize external preferences"),
    )
    .expect("simulate external writer");

    harness.with(|service| {
        service
            .edit(|preferences| preferences.files.sidebar.show_volumes = true)
            .expect("toggle show_volumes");
        service
            .edit(|preferences| preferences.files.auto_focus_search = true)
            .expect("follow-up edit");
    });
    harness.shutdown();

    assert_eq!(
        harness.with(|service| service.error().get_untracked()),
        None
    );
    let saved: Preferences =
        toml::from_str(&std::fs::read_to_string(&path).expect("read saved settings"))
            .expect("valid saved settings");
    assert!(saved.files.sidebar.show_volumes);
    assert!(saved.files.auto_focus_search);
    assert!(saved.files.show_hidden);
}

#[test]
fn corrupt_startup_config_reports_save_error_without_overwriting_disk() {
    let directory = tempfile::tempdir().expect("configuration directory");
    let path = directory.path().join("settings.toml");
    let corrupt = "[files.sidebar]\nshow_volumes = [broken\n";
    std::fs::write(&path, corrupt).expect("seed corrupt config");

    let mut startup = api::read_startup_from(path.clone());
    assert!(startup.notice.is_some());
    startup.preferences.appearance.source = theme::Source::Builtin;
    let _ = install();
    let supervisor = Supervisor::new(false).expect("preference test runtime");
    let scope = supervisor.scope();
    let root = Mounted::new();
    let service = root.with(|| {
        let app = Gds::install(scope.clone());
        app.appearance()
            .theme()
            .apply(&startup.preferences.appearance)
            .expect("initial theme");
        app.appearance()
            .theme()
            .configure_hot_reload(startup.preferences.appearance.clone());
        Service::install_loaded(startup, &scope)
    });

    root.with(|| {
        Gds::current().with(|| {
            service
                .edit(|preferences| preferences.files.sidebar.show_volumes = true)
                .expect("accept edit while disk is corrupt");
        });
    });

    supervisor.shutdown();
    for _ in 0..4 {
        flush();
    }

    let error = root.with(|| Gds::current().with(|| service.error().get_untracked()));
    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("invalid configuration")),
        "expected parse error on corrupt startup config, got {error:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("corrupt config remains intact"),
        corrupt
    );
    root.unmount();
}
