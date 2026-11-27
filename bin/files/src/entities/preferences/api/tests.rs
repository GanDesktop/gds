use std::fs;

use gds::appearance::theme;

use super::{Error, Preferences, Store, read_startup_from};

fn write_toml(path: &std::path::Path, preferences: &Preferences) {
    let encoded = toml::to_string_pretty(preferences).expect("serialize test preferences");
    fs::write(path, encoded).expect("write test preferences");
}

fn read_toml(path: &std::path::Path) -> Preferences {
    let text = fs::read_to_string(path).expect("read saved preferences");
    toml::from_str(&text).expect("parse saved preferences")
}

#[test]
fn external_change_reconciles_disjoint_fields_and_subsequent_saves_keep_external_edits() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let initial = Preferences::default();
    write_toml(&path, &initial);

    let mut startup = read_startup_from(path.clone());
    assert!(startup.notice.is_none());

    // Simulate another process (e.g. file picker) updating settings.toml on disk.
    let mut external = initial;
    external.files.show_hidden = true;
    external.terminal.font_size = 16.0;
    write_toml(&path, &external);

    // Local session toggles persistent volumes list without knowing about the external change.
    let mut local = startup.preferences.clone();
    local.files.sidebar.show_volumes = true;
    startup
        .store
        .save(&local)
        .expect("reconcile external modification on save");

    let after_first = read_toml(&path);
    assert!(after_first.files.sidebar.show_volumes);
    assert!(after_first.files.show_hidden);
    assert_eq!(after_first.terminal.font_size, 16.0);

    // A follow-up save in this session must not clobber the earlier external changes.
    local.files.auto_focus_search = true;
    startup
        .store
        .save(&local)
        .expect("follow-up save preserves external fields");

    let after_second = read_toml(&path);
    assert!(after_second.files.sidebar.show_volumes);
    assert!(after_second.files.auto_focus_search);
    assert!(after_second.files.show_hidden);
    assert_eq!(after_second.terminal.font_size, 16.0);
}

#[test]
fn external_create_and_delete_recover_cleanly_on_save() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");

    // Start with no settings.toml on disk; another process creates it before our first save.
    let mut startup = read_startup_from(path.clone());
    let mut external = Preferences::default();
    external.files.show_hidden = true;
    write_toml(&path, &external);

    let mut local = startup.preferences.clone();
    local.files.sidebar.show_volumes = true;
    startup
        .store
        .save(&local)
        .expect("merge against externally created file");
    let saved = read_toml(&path);
    assert!(saved.files.show_hidden);
    assert!(saved.files.sidebar.show_volumes);

    // File is removed externally before another edit; save recreates it cleanly.
    fs::remove_file(&path).expect("remove settings file externally");
    local.files.auto_focus_search = true;
    startup
        .store
        .save(&local)
        .expect("recreate externally removed file");
    let recreated = read_toml(&path);
    assert!(recreated.files.auto_focus_search);
}

#[test]
fn corrupt_disk_config_is_not_overwritten_and_recovers_after_repair() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let corrupt = "[files.sidebar]\nshow_volumes = [broken\n";
    fs::write(&path, corrupt).expect("write corrupt config");

    let mut startup = read_startup_from(path.clone());
    assert!(startup.notice.is_some());
    assert_eq!(startup.preferences, Preferences::default());

    let mut local = startup.preferences.clone();
    local.files.sidebar.show_volumes = true;
    let err = startup
        .store
        .save(&local)
        .expect_err("corrupt file must not be overwritten");
    assert!(matches!(err, Error::Parse { .. }));
    assert_eq!(fs::read_to_string(&path).expect("still corrupt"), corrupt);

    // Once the user repairs the syntax error on disk, the next save merges cleanly.
    let mut repaired = Preferences::default();
    repaired.files.show_hidden = true;
    write_toml(&path, &repaired);

    local.files.auto_focus_search = true;
    startup
        .store
        .save(&local)
        .expect("repaired file recovers and keeps pending local edits");
    let saved = read_toml(&path);
    assert!(saved.files.show_hidden);
    assert!(saved.files.sidebar.show_volumes);
    assert!(saved.files.auto_focus_search);
}

#[test]
fn runtime_fallback_baseline_does_not_overwrite_saved_system_theme_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let mut on_disk = Preferences::default();
    on_disk.appearance.source = theme::Source::System;
    write_toml(&path, &on_disk);

    let mut startup = read_startup_from(path.clone());
    startup.preferences.appearance.source = theme::Source::Builtin;
    startup
        .store
        .set_local_baseline(startup.preferences.clone());

    let mut local = startup.preferences.clone();
    local.files.sidebar.show_volumes = true;
    startup
        .store
        .save(&local)
        .expect("save non-theme toggle after runtime fallback");

    let saved = read_toml(&path);
    assert!(saved.files.sidebar.show_volumes);
    assert_eq!(saved.appearance.source, theme::Source::System);
}

#[cfg(unix)]
#[test]
fn symlinked_config_remains_protected_without_clobbering_target() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("managed.toml");
    let link = dir.path().join("settings.toml");
    write_toml(&target, &Preferences::default());
    symlink(&target, &link).expect("create managed symlink");

    let mut startup = read_startup_from(link);
    let mut local = startup.preferences.clone();
    local.files.sidebar.show_volumes = true;

    let err = startup
        .store
        .save(&local)
        .expect_err("managed symlink must still be rejected");
    assert!(matches!(
        err,
        Error::Persistence(gds::persistence::Error::Symlink(_))
    ));
    assert!(!read_toml(&target).files.sidebar.show_volumes);
}

#[test]
fn two_independent_stores_persist_disjoint_edits_without_conflict() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let initial = Preferences::default();
    write_toml(&path, &initial);

    let mut browser = Store::for_path(path.clone(), initial.clone());
    let mut picker = Store::for_path(path.clone(), initial.clone());

    let mut picker_prefs = initial.clone();
    picker_prefs.files.show_hidden = true;
    picker.save(&picker_prefs).expect("picker save succeeds");

    let mut browser_prefs = initial;
    browser_prefs.files.sidebar.show_volumes = true;
    browser
        .save(&browser_prefs)
        .expect("browser save reconciles picker edit");

    let final_disk = read_toml(&path);
    assert!(final_disk.files.show_hidden);
    assert!(final_disk.files.sidebar.show_volumes);
}
