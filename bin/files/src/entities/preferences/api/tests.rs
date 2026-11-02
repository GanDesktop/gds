use super::*;
use crate::shared::i18n::Language;
use gds::{
    appearance::theme::{ModePreference, Source},
    ui::time::Format,
};

fn fixture() -> tempfile::TempDir {
    tempfile::tempdir().expect("create preferences fixture")
}

fn application_path(root: &tempfile::TempDir) -> PathBuf {
    root.path().join("gds/files/settings.toml")
}

fn legacy_path(root: &tempfile::TempDir) -> PathBuf {
    root.path().join("gds/settings.toml")
}

#[test]
fn missing_configurations_use_defaults_without_creating_any_file() {
    let root = fixture();
    let application = application_path(&root);
    let legacy = legacy_path(&root);
    let loaded =
        load_startup_from(&application, &legacy).expect("load missing application preferences");
    assert_eq!(loaded.preferences, Preferences::default());
    assert!(loaded.migration.is_none());
    assert!(!application.exists());
    assert!(!legacy.exists());
}

#[test]
fn saves_and_reloads_the_complete_preferences_including_cleared_language() {
    let root = fixture();
    let path = application_path(&root);
    let mut preferences = Preferences::default();
    preferences.appearance.source = Source::Qt6ct;
    preferences.appearance.builtin = gds::appearance::theme::BuiltinTheme::Neon;
    preferences.appearance.mode = ModePreference::Light;
    preferences.appearance.content_opacity = 0.75;
    preferences.appearance.sidebar_opacity = 0.85;
    preferences.language = Some(Language::Chinese);
    preferences.time_format = Format::Absolute;
    preferences.animations_enabled = Some(false);
    preferences.font.family = Some("Noto Sans".to_owned());
    preferences.font.size = 18.0;
    preferences.files.show_hidden = true;
    preferences.terminal.shell = Some("/usr/bin/fish".into());
    preferences.terminal.shell_args =
        vec!["--private".into(), "literal argument with spaces".into()];
    preferences.terminal.login_shell = true;
    preferences.terminal.font_family = "Terminal Mono".into();
    preferences.terminal.font_size = 16.;
    preferences.terminal.scrollback_lines = 25_000;
    preferences.terminal.cursor_blink = false;
    preferences.terminal.alt_sends_escape = false;
    preferences.files.terminal_content_height = Some(420.);
    preferences.files.sidebar.tabs_height = Some(284.);
    preferences.files.sidebar.volumes_collapsed = true;
    preferences.files.sidebar.locations_collapsed = true;
    preferences.files.sidebar.show_volumes = true;
    preferences
        .files
        .volume_mount_points
        .insert("filesystem-identity".into(), "/nix/store".into());

    save_to(&path, &preferences).expect("save preferences");
    let reloaded = load_from(&path).expect("reload preferences");
    assert_eq!(reloaded, preferences);
    assert_eq!(reloaded.time_format, Format::Absolute);
    assert_eq!(reloaded.font.family.as_deref(), Some("Noto Sans"));
    assert_eq!(reloaded.font.size, 18.0);

    preferences.language = None;
    preferences.files.show_hidden = false;
    save_to(&path, &preferences).expect("replace preferences");
    let reloaded = load_from(&path).expect("reload replacement");
    assert_eq!(reloaded, preferences);
    assert_eq!(reloaded.time_format, Format::Absolute);
}

#[test]
fn omitted_fields_receive_defaults_but_malformed_configuration_is_an_error() {
    let root = fixture();
    let path = root.path().join("settings.toml");
    fs::write(&path, "[files]\nshow_hidden = true\n").expect("write partial preferences");
    let preferences = load_from(&path).expect("read supported partial preferences");
    assert!(preferences.files.show_hidden);
    assert_eq!(preferences.appearance, Preferences::default().appearance);
    assert_eq!(preferences.language, None);
    assert_eq!(preferences.time_format, Format::Relative);
    assert!(preferences.animations_enabled.unwrap_or(true));
    assert_eq!(preferences.font, Preferences::default().font);
    assert_eq!(
        preferences.terminal,
        gds::ui::terminal::Preferences::default()
    );
    assert_eq!(preferences.files.terminal_content_height, None);

    let invalid = "[files\nshow_hidden = ?";
    fs::write(&path, invalid).expect("write malformed preferences");
    assert!(matches!(load_from(&path), Err(Error::Parse { .. })));
    assert_eq!(
        fs::read_to_string(&path).expect("read unchanged file"),
        invalid
    );
}

#[test]
fn partial_terminal_preferences_keep_shared_defaults_and_literal_arguments() {
    let root = fixture();
    let path = application_path(&root);
    fs::create_dir_all(path.parent().expect("preferences parent")).expect("create preferences dir");
    fs::write(
        &path,
        "[terminal]\nlogin_shell = true\nshell_args = ['--flag', 'two words', '$(not executed)']\n",
    )
    .expect("write partial terminal preferences");
    let preferences = load_from(&path).expect("load partial terminal preferences");
    assert!(preferences.terminal.login_shell);
    assert_eq!(preferences.terminal.shell, None);
    assert_eq!(
        preferences.terminal.font_size,
        gds::ui::terminal::Preferences::default().font_size
    );
    assert_eq!(
        preferences.terminal.shell_args,
        ["--flag", "two words", "$(not executed)"]
    );
    save_to(&path, &preferences).expect("save terminal configuration");
    assert_eq!(
        load_from(&path).expect("reload terminal configuration"),
        preferences
    );
}

#[test]
fn legacy_preferences_are_imported_once_without_touching_the_old_file() {
    let root = fixture();
    let application = application_path(&root);
    let legacy = legacy_path(&root);
    let legacy_contents = "[files]\nshow_hidden = true\n";
    fs::create_dir_all(legacy.parent().expect("legacy directory"))
        .expect("create legacy directory");
    fs::write(&legacy, legacy_contents).expect("write legacy preferences");

    let imported = load_startup_from(&application, &legacy).expect("import legacy preferences");
    assert!(imported.migration.is_none());
    assert!(imported.preferences.files.show_hidden);
    assert_eq!(
        load_from(&application).expect("read imported preferences"),
        imported.preferences
    );
    assert_eq!(
        fs::read_to_string(&legacy).expect("read unchanged legacy file"),
        legacy_contents
    );

    // The application configuration now wins over any legacy change.
    let mut application_preferences = imported.preferences.clone();
    application_preferences.files.show_hidden = false;
    save_to(&application, &application_preferences).expect("save application preferences");
    fs::write(&legacy, "[files]\nshow_hidden = true\n").expect("update legacy preferences");
    let startup = load_startup_from(&application, &legacy).expect("load application preferences");
    assert!(!startup.preferences.files.show_hidden);
    assert!(startup.migration.is_none());
    assert_eq!(
        fs::read_to_string(&legacy).expect("read untouched legacy file"),
        "[files]\nshow_hidden = true\n"
    );
}

#[test]
fn malformed_legacy_preferences_are_reported_without_writing_application_defaults() {
    let root = fixture();
    let application = application_path(&root);
    let legacy = legacy_path(&root);
    let invalid = "[files\nshow_hidden = ?";
    fs::create_dir_all(legacy.parent().expect("legacy directory"))
        .expect("create legacy directory");
    fs::write(&legacy, invalid).expect("write malformed legacy preferences");

    assert!(matches!(
        load_startup_from(&application, &legacy),
        Err(Error::Parse { .. })
    ));
    assert!(!application.exists());
    assert_eq!(
        fs::read_to_string(&legacy).expect("read unchanged legacy file"),
        invalid
    );
}

#[cfg(unix)]
#[test]
fn a_declarative_legacy_link_is_read_and_the_managed_source_is_untouched() {
    use std::os::unix::fs::symlink;

    let root = fixture();
    let application = application_path(&root);
    let legacy = legacy_path(&root);
    let target = root.path().join("managed-legacy.toml");
    let contents = "[files]\nshow_hidden = true\n";
    fs::write(&target, contents).expect("write managed legacy preferences");
    fs::create_dir_all(legacy.parent().expect("legacy directory"))
        .expect("create legacy directory");
    symlink(&target, &legacy).expect("link declarative legacy preferences");

    let imported = load_startup_from(&application, &legacy).expect("import linked preferences");
    assert!(imported.preferences.files.show_hidden);
    assert!(imported.migration.is_none());
    assert_eq!(fs::read_link(&legacy).expect("retain legacy link"), target);
    assert_eq!(
        fs::read_to_string(&target).expect("read unchanged managed source"),
        contents
    );
    assert!(
        load_from(&application)
            .expect("read imported preferences")
            .files
            .show_hidden
    );
}

#[cfg(unix)]
#[test]
fn a_failed_import_keeps_the_legacy_preferences_for_this_run() {
    use std::os::unix::fs::PermissionsExt;

    let root = fixture();
    let application = application_path(&root);
    let legacy = legacy_path(&root);
    fs::create_dir_all(legacy.parent().expect("legacy directory"))
        .expect("create legacy directory");
    fs::write(&legacy, "[files]\nshow_hidden = true\n").expect("write legacy preferences");
    let directory = application.parent().expect("application directory");
    fs::create_dir_all(directory).expect("create application directory");
    fs::set_permissions(directory, fs::Permissions::from_mode(0o555))
        .expect("make application directory read-only");

    let loaded = load_startup_from(&application, &legacy).expect("load legacy preferences");
    assert!(loaded.preferences.files.show_hidden);
    assert!(matches!(
        loaded.migration,
        Some(Error::Persistence(gds::persistence::Error::ReadOnly(_)))
    ));
    assert!(!application.exists());
    assert_eq!(
        fs::read_to_string(&legacy).expect("read unchanged legacy file"),
        "[files]\nshow_hidden = true\n"
    );

    fs::set_permissions(directory, fs::Permissions::from_mode(0o755))
        .expect("restore application directory permissions");
}

#[test]
fn an_import_never_overwrites_a_configuration_created_concurrently() {
    let root = fixture();
    let application = application_path(&root);
    fs::create_dir_all(application.parent().expect("application directory"))
        .expect("create application directory");
    fs::write(&application, "[files]\nshow_hidden = false\n")
        .expect("write concurrent preferences");

    assert!(matches!(
        write_atomic(&application, &Preferences::default(), false),
        Err(Error::Persistence(gds::persistence::Error::Conflict(_)))
    ));
    assert_eq!(
        fs::read_to_string(&application).expect("read concurrent preferences"),
        "[files]\nshow_hidden = false\n"
    );
}

#[test]
fn an_import_that_lost_to_a_concurrent_configuration_uses_that_configuration() {
    let root = fixture();
    let application = application_path(&root);
    let legacy = legacy_path(&root);
    fs::create_dir_all(application.parent().expect("application directory"))
        .expect("create application directory");
    let concurrent_contents = "[files]\nshow_hidden = false\n";
    fs::write(&application, concurrent_contents).expect("write concurrent preferences");
    let legacy_contents = "[files]\nshow_hidden = true\n";
    fs::create_dir_all(legacy.parent().expect("legacy directory"))
        .expect("create legacy directory");
    fs::write(&legacy, legacy_contents).expect("write legacy preferences");
    let legacy_preferences = read(&legacy).expect("read legacy preferences");

    // A concurrent configuration wins the no-clobber copy, so the import must
    // resolve to the file on disk rather than the values it tried to write.
    let lost = write_atomic(&application, &legacy_preferences, false)
        .expect_err("refuse to replace the concurrent configuration");
    assert!(matches!(
        &lost,
        Error::Persistence(gds::persistence::Error::Conflict(_))
    ));
    let loaded =
        import_outcome(&application, legacy_preferences, lost).expect("use concurrent preferences");

    assert!(!loaded.preferences.files.show_hidden);
    assert!(loaded.migration.is_none());
    assert_eq!(
        fs::read_to_string(&application).expect("read concurrent preferences"),
        concurrent_contents
    );
    assert_eq!(
        fs::read_to_string(&legacy).expect("read unchanged legacy file"),
        legacy_contents
    );
}

#[test]
fn replacement_leaves_only_the_complete_configuration_behind() {
    let root = fixture();
    let path = application_path(&root);
    save_to(&path, &Preferences::default()).expect("save preferences");
    let mut preferences = Preferences::default();
    preferences.files.show_hidden = true;
    save_to(&path, &preferences).expect("replace preferences");

    let directory = path.parent().expect("application directory");
    let entries: Vec<_> = fs::read_dir(directory)
        .expect("read application directory")
        .map(|entry| {
            entry
                .expect("read directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(entries.len(), 2);
    assert!(entries.contains(&"settings.toml".to_owned()));
    assert!(entries.contains(&"settings.toml.lock".to_owned()));
    assert_eq!(load_from(&path).expect("reload preferences"), preferences);
}

#[cfg(unix)]
#[test]
fn readonly_configuration_is_not_replaced_even_when_the_directory_is_writable() {
    use std::os::unix::fs::PermissionsExt;

    let root = fixture();
    let path = root.path().join("settings.toml");
    let contents = "[files]\nshow_hidden = true\n";
    fs::write(&path, contents).expect("write managed preferences");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444))
        .expect("make preferences readonly");

    assert!(matches!(
        save_to(&path, &Preferences::default()),
        Err(Error::Persistence(gds::persistence::Error::ReadOnly(_)))
    ));
    assert_eq!(
        fs::read_to_string(&path).expect("read unchanged file"),
        contents
    );
    assert!(
        fs::metadata(&path)
            .expect("read permissions")
            .permissions()
            .readonly()
    );
}

#[cfg(unix)]
#[test]
fn declarative_symlinks_can_be_read_but_cannot_be_replaced() {
    use std::os::unix::fs::symlink;

    let root = fixture();
    let target = root.path().join("managed.toml");
    let path = root.path().join("settings.toml");
    let contents = "[files]\nshow_hidden = true\n";
    fs::write(&target, contents).expect("write managed target");
    symlink(&target, &path).expect("link declarative preferences");

    assert!(
        load_from(&path)
            .expect("read declarative preferences")
            .files
            .show_hidden
    );
    assert!(matches!(
        save_to(&path, &Preferences::default()),
        Err(Error::Persistence(gds::persistence::Error::Symlink(_)))
    ));
    assert_eq!(fs::read_link(&path).expect("retain symlink"), target);
    assert_eq!(
        fs::read_to_string(&target).expect("read unchanged target"),
        contents
    );
}

#[cfg(unix)]
#[test]
fn a_dangling_configuration_link_is_reported_instead_of_reset_to_defaults() {
    use std::os::unix::fs::symlink;

    let root = fixture();
    let target = root.path().join("missing-managed.toml");
    let path = root.path().join("settings.toml");
    symlink(&target, &path).expect("create dangling configuration link");

    assert!(matches!(load_from(&path), Err(Error::Read { .. })));
    assert!(matches!(
        save_to(&path, &Preferences::default()),
        Err(Error::Persistence(gds::persistence::Error::Symlink(_)))
    ));
    assert_eq!(fs::read_link(&path).expect("retain dangling link"), target);
    assert!(!target.exists());
}
